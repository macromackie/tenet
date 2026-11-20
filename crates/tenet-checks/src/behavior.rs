//! Turn an unbound English contract into a frozen, independently replayable observation.

mod environment;
mod process;
mod request;
#[cfg(test)]
mod tests;

use alloc::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tenet_engine::{Plan, review::Checked};

/// Observation outcome reported by one executable behavior check.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Failed,
    Passed,
    Unresolved,
}

/// One executable behavior-check finding returned by the isolated checker worker.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Finding {
    pub check: String,
    pub contract: String,
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_requests: Option<usize>,
    pub message: String,
    pub outcome: Outcome,
    pub requested: bool,
    pub reused: bool,
    pub subject: Option<PathBuf>,
}

const FILES: &[(&str, &str)] = &[
    ("worker.mjs", include_str!("../../../checker/worker.bundle")),
    (
        "checker-prompt.md",
        include_str!("../../../checker/checker-prompt.md"),
    ),
    (
        "package.json",
        include_str!("../../../checker/package.json"),
    ),
    (
        "package-lock.json",
        include_str!("../../../checker/package-lock.json"),
    ),
];

enum Operation<'plan> {
    Assess {
        fresh: bool,
        max_generations: usize,
        checked: &'plan Checked,
        known: &'plan [Finding],
    },
    Inspect,
}

/// Generate, qualify, and replay behavior checks for contracts touched by `plan`.
#[inline]
#[must_use]
pub fn assess(
    plan: &Plan,
    state: &Path,
    max_generations: usize,
    fresh: bool,
    checked: &Checked,
    known: &[Finding],
    on_progress: Option<&mut dyn FnMut(&str)>,
) -> Vec<Finding> {
    run(
        plan,
        state,
        &Operation::Assess {
            fresh,
            max_generations,
            checked,
            known,
        },
        on_progress,
    )
}

/// Inspect cached behavior-check receipts for `plan` without running probes or spending requests.
#[inline]
#[must_use]
pub fn inspect(plan: &Plan, state: &Path) -> Vec<Finding> {
    run(plan, state, &Operation::Inspect, None)
}

fn run(
    plan: &Plan,
    state: &Path,
    operation: &Operation<'_>,
    on_progress: Option<&mut dyn FnMut(&str)>,
) -> Vec<Finding> {
    let mut scopes = BTreeMap::new();
    for contract in &plan.contracts {
        // Unchanged, unrelated packages never become visible on a PR or a selected-path check.
        let changed_contract = plan.contract_changes.contains(&contract.path);
        if !changed_contract
            && !plan
                .files
                .iter()
                .chain(&plan.deleted)
                .any(|file| contract.is_governing(file))
        {
            continue;
        }
        let _prior = scopes.insert(contract.name.as_str(), contract);
    }
    if matches!(operation, Operation::Inspect) && !state.join("agent-checks").exists() {
        scopes.retain(|_, contract| {
            plan.root
                .join(contract.path.with_file_name("agent-checks.lock.json"))
                .exists()
        });
    }
    if scopes.is_empty() {
        return Vec::new();
    }
    let result = request::dispatch(plan, state, &scopes, operation, on_progress);
    match result {
        Ok(findings) => findings,
        Err(error) => {
            scopes
                .values()
                .map(|contract| {
                    Finding {
                        check: "discovery".into(),
                        contract: contract.name.clone(),
                        cost_usd: None,
                        generation_requests: None,
                        message: format!("Could not start the checker worker: {error}"),
                        outcome: Outcome::Unresolved,
                        requested: false,
                        reused: false,
                        subject: None,
                    }
                })
                .collect()
        },
    }
}
