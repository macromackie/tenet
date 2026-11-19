//! Report executable examples alongside the semantic review without changing its recorded verdict.

mod declared;
mod native;

use std::{
    io::{self, Write},
    path::Path,
};

use anyhow::Context as _;
use ev_grep_jev::Provider;
use serde_json::{Value, json};
use tenet_engine::{
    Plan,
    Selection,
    mechanical::has_local_results,
    review::{Checked, CurrentContract, OverallStatus},
};

use crate::{
    agent_checks::{self, Finding, Outcome},
    cli::{Check, ReporterKind},
    error::CommandError,
    output,
    report_format::format_painted,
};

/// Results for the standing path selection in a stored review. Inspection never runs a checker.
pub(crate) fn inspect(
    root: &Path,
    state: &Path,
    entry: &CurrentContract<'_>,
    include_local: bool,
) -> Result<Vec<Finding>, CommandError> {
    let mut rows = native::read(root, state, &entry.contract, &entry.input)?;
    if entry.contract.obligations.is_empty()
        && !state.join("agent-checks").is_dir()
        && !root
            .join(entry.contract.path.with_file_name("agent-checks.lock.json"))
            .is_file()
    {
        return Ok(rows);
    }
    let selection = Selection {
        base: entry.selection.base_oid.clone(),
        paths: entry.selection.paths.clone(),
        contract: Some(entry.contract.name.clone()),
    };
    let plan = tenet_engine::plan(root, &selection)?
        .with_context_profile(entry.selection.context_profile)
        .with_context(&entry.selection.context)?;
    if include_local {
        rows.extend(declared::inspect(&plan, state)?);
    }
    rows.extend(agent_checks::inspect(&plan, state));
    Ok(rows)
}

/// Generate checks only against the configured `OpenRouter` service and within the unused request
/// budget.
pub(crate) async fn assess(
    plan: &Plan,
    state: &Path,
    check: &Check,
    checked: &Checked,
) -> Result<Vec<Finding>, CommandError> {
    if check.input.snapshot.is_some() {
        return Ok(Vec::new());
    }
    if check.no_model
        || check.evidence.is_some()
        || check.run.endpoint.is_some()
        || check.run.provider != Provider::OpenRouter
    {
        let mut rows = if check.no_model && check.evidence.is_none() {
            agent_checks::replay(plan, state)
        } else {
            agent_checks::inspect(plan, state)
        };
        if !check.final_check && has_local_results(state) {
            rows.extend(declared::inspect(plan, state)?);
        }
        for entry in &checked.contracts {
            if let Some(contract) = plan
                .contracts
                .iter()
                .find(|contract| contract.name == entry.contract)
            {
                rows.extend(native::read(&plan.root, state, contract, &entry.input)?);
            }
        }
        return Ok(rows);
    }
    let mut rows = native::assess(plan, state, checked).await?;
    rows.extend(declared::inspect(plan, state)?);
    let limit = check
        .run
        .request_budget()
        .context("convert request budget")?;
    let probes = agent_checks::assess(
        plan,
        state,
        limit.saturating_sub(checked.summary.requests),
        check.fresh,
        checked,
        &rows,
    );
    if check.final_check {
        // Final review carries the same project checks, including imported receipts, in its checks
        // array.
        rows.retain(|row| !row.check.starts_with("project-"));
    }
    rows.extend(probes);
    Ok(rows)
}

/// Account for generation separately from the Jev request receipts. A missing price stays unknown.
pub(crate) fn summarize_generation(rows: &[Finding]) -> Value {
    let attempts = rows.iter().filter(|row| row.requested);
    let requests = attempts.clone().count();
    let cost: Option<f64> = attempts.map(|row| row.cost_usd).sum();
    json!({"requests": requests, "returned_cost_usd": cost})
}

pub(crate) fn summarize_status(rows: &[Finding]) -> OverallStatus {
    rows.iter()
        .map(|row| {
            match row.outcome {
                Outcome::Passed => OverallStatus::Passed,
                Outcome::Failed => OverallStatus::Failed,
                Outcome::Unresolved => OverallStatus::Unresolved,
            }
        })
        .max()
        .unwrap_or(OverallStatus::Passed)
}

pub(crate) fn write_rows(out: &mut impl Write, rows: &[Finding]) -> io::Result<()> {
    for row in rows {
        let label = match row.outcome {
            Outcome::Passed => "PASS",
            Outcome::Failed => "FAIL",
            Outcome::Unresolved => "REVIEW",
        };
        writeln!(
            out,
            "{label}  {}/{}",
            row.contract.escape_debug(),
            row.check.escape_debug()
        )?;
        writeln!(out, "      {}", row.message.escape_debug())?;
    }
    Ok(())
}

pub(crate) fn emit(rows: &[Finding], kind: ReporterKind) -> Result<(), CommandError> {
    match kind {
        ReporterKind::Jsonl => {
            output::write_document(
                &mut output::open_stdout().lock(),
                tenet_engine::EVENTS_VERSION,
                json!({"type": "behavior_checks", "checks": rows, "generation": summarize_generation(rows)}),
            )?;
        },
        ReporterKind::Verbose => write_rows(&mut output::open_stderr().lock(), rows)?,
        ReporterKind::Default => {
            let mut out = output::open_stderr().lock();
            for row in rows {
                let label = match row.outcome {
                    Outcome::Passed => continue,
                    Outcome::Failed => "FAIL",
                    Outcome::Unresolved => "REVIEW",
                };
                let location = row
                    .subject
                    .as_deref()
                    .map(|path| format!("  {}", path.display().to_string().escape_debug()))
                    .unwrap_or_default();
                writeln!(
                    out,
                    "  {}  {}/{}{location}",
                    format_painted(label),
                    row.contract.escape_debug(),
                    row.check.escape_debug()
                )?;
                writeln!(out, "        {}", row.message.escape_debug())?;
            }
        },
    }
    Ok(())
}
