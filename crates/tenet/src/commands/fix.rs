mod audit;
mod contracts;

use std::{io::Write as _, path::Path};

use anyhow::Context as _;
use tenet_checks::{behavior::AuditFixResponse, fixes};
use tenet_engine::{Selection, discover_contracts};

use super::{audit as audit_cmd, prepare::resolve_pr_base};
use crate::{
    cli::{ExitStatus, Fix},
    error::CommandError,
    output,
};

struct RuntimeSummary {
    note: Option<String>,
    response: Option<AuditFixResponse>,
    unpatched_failures: usize,
}

pub(super) async fn run(root: &Path, state: &Path, args: &Fix) -> Result<ExitStatus, CommandError> {
    if args.from_audit {
        return audit::run(root, state, args);
    }
    let mut native_report = run_native_fixes(root, args).await?;
    let runtime = run_runtime_fixes(root, state, args, &mut native_report)?;
    let status = compute_combined_status(&native_report, &runtime);
    if args.json {
        write_combined_json(&native_report, &runtime, status)?;
    } else {
        write_combined_terminal(&native_report, &runtime)?;
    }
    Ok(status)
}

fn compute_combined_status(native: &fixes::Report, runtime: &RuntimeSummary) -> ExitStatus {
    let runtime_status = runtime.response.as_ref().map_or_else(
        || {
            if runtime.unpatched_failures > 0 {
                ExitStatus::Failed
            } else {
                ExitStatus::Passed
            }
        },
        |response| audit::read_fix_exit_status(&response.outcomes),
    );
    if !native.unresolved.is_empty() || runtime_status == ExitStatus::Unresolved {
        return ExitStatus::Unresolved;
    }
    if native.verified.iter().any(|check| !check.passed) || runtime_status == ExitStatus::Failed {
        return ExitStatus::Failed;
    }
    ExitStatus::Passed
}

async fn run_native_fixes(root: &Path, args: &Fix) -> Result<fixes::Report, CommandError> {
    if discover_contracts(root)?.is_empty() {
        return Ok(fixes::Report::default());
    }
    let selection = Selection {
        base: match args.base.as_deref() {
            Some(reference) => Some(reference.to_owned()),
            None => resolve_pr_base(root)?,
        },
        contract: args.contract.clone(),
        paths: args.paths.clone(),
    };
    let plan = tenet_engine::plan(root, &selection)?;
    Ok(fixes::apply(&plan).await.map_err(anyhow::Error::from)?)
}

fn run_runtime_fixes(
    root: &Path,
    state: &Path,
    args: &Fix,
    native_report: &mut fixes::Report,
) -> Result<RuntimeSummary, CommandError> {
    let plan = contracts::prepare_runtime_fix_plan(root, state, args)?;
    if plan.groups.is_empty() {
        return Ok(RuntimeSummary {
            note: None,
            response: None,
            unpatched_failures: 0,
        });
    }
    let failing_count: usize = plan.groups.iter().map(|group| group.witnesses.len()).sum();
    if !audit_cmd::has_openrouter_key() {
        return Ok(RuntimeSummary {
            note: Some(format!(
                "Set OPENROUTER_API_KEY to auto-patch {failing_count} failing runtime check(s)."
            )),
            response: None,
            unpatched_failures: failing_count,
        });
    }
    let max_requests = usize::try_from(args.max_requests).context("convert fix request budget")?;
    let (response, changes) = audit::execute_audit_fix(
        root,
        state,
        args.json,
        false,
        max_requests,
        plan.groups,
        plan.runtime_paths,
    )?;
    native_report.changes.extend(changes);
    Ok(RuntimeSummary {
        note: None,
        response: Some(response),
        unpatched_failures: 0,
    })
}

#[expect(
    clippy::indexing_slicing,
    reason = "The JSON value is constructed as an object before optional runtime fields are added."
)]
fn write_combined_json(
    report: &fixes::Report,
    runtime: &RuntimeSummary,
    status: ExitStatus,
) -> Result<(), CommandError> {
    let mut document = serde_json::json!({
        "changes": report.changes,
        "manual": report.manual,
        "checks": report.verified,
        "unresolved": report.unresolved,
        "exit_code": u8::from(status),
    });
    if let Some(response) = &runtime.response {
        document["runtime_outcomes"] = serde_json::json!(response.outcomes);
        document["requests"] = serde_json::json!(response.requests);
        document["cost_usd"] = serde_json::json!(response.cost_usd);
    }
    if let Some(note) = &runtime.note {
        document["note"] = serde_json::json!(note);
    }
    output::write_document(&mut output::open_stdout(), 1, document)?;
    Ok(())
}

fn write_combined_terminal(
    report: &fixes::Report,
    runtime: &RuntimeSummary,
) -> Result<(), CommandError> {
    let mut out = output::open_stdout().lock();
    for change in &report.changes {
        writeln!(out, "{}", change.patch)?;
    }
    for row in &report.unresolved {
        writeln!(
            out,
            "Unresolved  {} ({}) — {}",
            row.contract, row.tool, row.message
        )?;
    }
    match (
        !report.unresolved.is_empty(),
        !report.verified.is_empty(),
        runtime.response.is_none() && runtime.note.is_none(),
    ) {
        (true, ..) => writeln!(out, "No fixes applied.")?,
        (false, true, _) => {
            writeln!(out, "{} files updated.", report.changes.len())?;
            for check in &report.verified {
                let observation = if check.passed {
                    "Passed"
                } else {
                    "Findings remain"
                };
                writeln!(out, "{observation}  {} ({})", check.contract, check.tool)?;
                if let Some(finding) = &check.finding {
                    writeln!(out, "  {finding}")?;
                }
            }
        },
        (false, false, true) => {
            writeln!(out, "No safe native fixer applies to the selected files.")?;
        },
        (false, false, false) => {},
    }
    if let Some(response) = &runtime.response {
        audit::write_fix_terminal_outcomes(&mut out, response, report.changes.len())?;
    }
    if let Some(note) = &runtime.note {
        writeln!(out, "{note}")?;
    }
    if !report.manual.is_empty() {
        writeln!(out, "No native fix for {}.", report.manual.join(", "))?;
    }
    writeln!(out, "Run tenet check to review the resulting code.")?;
    Ok(())
}
