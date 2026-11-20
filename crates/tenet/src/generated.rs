//! Report executable examples alongside the semantic review without changing its recorded verdict.

mod declared;
mod native;

use alloc::collections::BTreeMap;
use std::{
    io::{self, Write},
    path::Path,
};

use anyhow::Context as _;
use ev_grep_jev::Provider;
use serde_json::{Value, json};
use tenet_checks::{
    behavior::{self as agent_checks, Finding, Outcome},
    mechanical::has_local_results,
};
use tenet_engine::{
    Plan,
    Selection,
    review::{Checked, CurrentContract, OverallStatus},
};

use crate::{
    cli::{Check, ReporterKind},
    error::CommandError,
    output,
    report_format::{
        DiagnosticBlock,
        format_dim,
        format_display,
        format_marker,
        format_painted,
        write_diagnostic_block,
    },
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
#[expect(
    clippy::future_not_send,
    reason = "The CLI drives behavior-check progress callbacks on one thread."
)]
pub(crate) async fn assess(
    plan: &Plan,
    state: &Path,
    check: &Check,
    checked: &Checked,
    on_progress: Option<&mut dyn FnMut(&str)>,
) -> Result<Vec<Finding>, CommandError> {
    if check.input.snapshot.is_some() {
        return Ok(Vec::new());
    }
    if check.evidence.is_some()
        || check.run.endpoint.is_some()
        || check.run.provider != Provider::OpenRouter
    {
        let mut rows = agent_checks::inspect(plan, state);
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
        on_progress,
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
#[expect(
    clippy::float_arithmetic,
    reason = "Adding positive zero normalizes IEEE-754 negative zero before JSON serialization."
)]
pub(crate) fn summarize_generation(rows: &[Finding]) -> Value {
    let attempts = rows.iter().filter(|row| row.requested);
    let requests: usize = attempts
        .clone()
        .map(|row| row.generation_requests.unwrap_or(1))
        .sum();
    let cost: Option<f64> = if requests == 0 {
        Some(0.0_f64)
    } else {
        attempts
            .map(|row| row.cost_usd)
            .sum::<Option<f64>>()
            .map(|total| total + 0.0_f64)
    };
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

fn write_finding(out: &mut impl Write, label: &str, row: &Finding) -> io::Result<()> {
    let title = format!("{}/{}", row.contract, row.check);
    let subject = row
        .subject
        .as_deref()
        .map(|path| path.display().to_string());
    if label == "PASS" {
        let location = subject
            .as_deref()
            .map(|path| format!("  {}", format_dim(&format_display(path))))
            .unwrap_or_default();
        writeln!(
            out,
            "  {} {}  {}{location}",
            format_marker(label),
            format_painted(label),
            format_display(&title),
        )?;
        return writeln!(out, "        {}", format_dim(&format_display(&row.message)));
    }
    write_diagnostic_block(
        out,
        &DiagnosticBlock {
            help: None,
            label,
            location: subject.as_deref(),
            message: &row.message,
            meta: None,
            title: &title,
        },
    )
}

pub(crate) fn write_rows(out: &mut impl Write, rows: &[Finding]) -> io::Result<()> {
    for row in rows {
        let label = match row.outcome {
            Outcome::Passed => "PASS",
            Outcome::Failed => "FAIL",
            Outcome::Unresolved => "REVIEW",
        };
        write_finding(out, label, row)?;
    }
    Ok(())
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "Passing check counts are bounded by the rows returned in one run."
)]
fn write_default_rows(out: &mut impl Write, rows: &[Finding]) -> io::Result<()> {
    let mut passed_counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut wrote_diagnostic = false;
    for row in rows {
        let label = match row.outcome {
            Outcome::Passed => {
                *passed_counts.entry(row.contract.as_str()).or_default() += 1;
                continue;
            },
            Outcome::Failed => "FAIL",
            Outcome::Unresolved => "REVIEW",
        };
        write_finding(out, label, row)?;
        wrote_diagnostic = true;
    }
    if wrote_diagnostic && !passed_counts.is_empty() {
        writeln!(out)?;
    }
    for (contract, count) in passed_counts {
        let noun = if count == 1 {
            "behavior check passed"
        } else {
            "behavior checks passed"
        };
        writeln!(
            out,
            "  {} {}  {}  {}",
            format_marker("PASS"),
            format_painted("PASS"),
            format_display(contract),
            format_dim(&format!("{count} {noun}"))
        )?;
    }
    Ok(())
}

pub(crate) fn emit(rows: &[Finding], kind: ReporterKind) -> Result<(), CommandError> {
    match kind {
        ReporterKind::Jsonl => {
            output::write_document(
                &mut output::open_stdout().lock(),
                tenet_engine::EVENTS_VERSION,
                json!({
                    "type": "behavior_checks",
                    "checks": rows,
                    "generation": summarize_generation(rows)
                }),
            )?;
        },
        ReporterKind::Verbose => write_rows(&mut output::open_stderr().lock(), rows)?,
        ReporterKind::Default => write_default_rows(&mut output::open_stderr().lock(), rows)?,
    }
    Ok(())
}
