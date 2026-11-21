//! Report executable examples alongside the semantic review without changing its recorded verdict.

mod declared;
mod native;
mod render;

use alloc::collections::BTreeMap;
use std::path::Path;

use anyhow::Context as _;
use ev_grep_jev::Provider;
pub(crate) use render::{emit, write_section};
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

use crate::{cli::Check, error::CommandError};

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
    let decisions = entry.assessment.map(|assessment| {
        assessment
            .results
            .iter()
            .map(|result| (result.path.clone(), result.status))
            .collect::<BTreeMap<_, _>>()
    });
    rows.extend(agent_checks::inspect_with_decisions(
        &plan,
        state,
        decisions.as_ref(),
    ));
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

/// Account for generation and failure explanation separately from the Jev request receipts. A
/// missing price stays unknown.
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
    let explanations = rows
        .iter()
        .filter(|row| row.explanation_requests.is_some_and(|count| count > 0));
    let explanation_requests: usize = explanations
        .clone()
        .map(|row| row.explanation_requests.unwrap_or(0))
        .sum();
    let explanation_cost: Option<f64> = if explanation_requests == 0 {
        Some(0.0_f64)
    } else {
        explanations
            .map(|row| row.explanation_cost_usd)
            .sum::<Option<f64>>()
            .map(|total| total + 0.0_f64)
    };
    json!({
        "requests": requests,
        "returned_cost_usd": cost,
        "explanation_requests": explanation_requests,
        "explanation_cost_usd": explanation_cost,
    })
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
