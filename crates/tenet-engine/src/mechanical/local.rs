mod assessment;
mod binding;
mod process;
mod qualification;
mod store;

use std::{
    io,
    path::{Path, PathBuf},
};

use assessment::{recheck_inputs, report};
use binding::{Binding, Protocol, Snapshot};
pub use qualification::{Qualification, QualificationCase, qualify};
use serde::{Deserialize, Serialize};
use store::Record;
use tenet_contracts::Contract;

use super::{CheckName, Evidence, Freshness, Outcome, ReceiptError};
use crate::{Plan, Selection, plan};

const DIRECTORY: &str = "checks";

/// Whether a requested run may reuse a decisive, explicitly cacheable observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalMode {
    Fresh,
    Reuse,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    finding: Option<super::Finding>,
    message: String,
    outcome: Outcome,
}

impl Observation {
    fn mark_unresolved(message: impl Into<String>) -> Self {
        Self {
            finding: None,
            message: message.into(),
            outcome: Outcome::Unresolved,
        }
    }
}

fn report_storage_error(path: &Path, source: io::Error) -> ReceiptError {
    ReceiptError::Local {
        path: path.to_owned(),
        source,
    }
}

fn select_checks<'tree>(
    tree: &'tree Plan,
    selector: Option<&str>,
) -> Result<Vec<(&'tree Contract, &'tree str)>, ReceiptError> {
    let mut found = Vec::new();
    for contract in &tree.contracts {
        for name in &contract.obligations {
            if selector.is_none_or(|key| key == format!("{}/{name}", contract.name)) {
                found.push((contract, name.as_str()));
            }
        }
    }
    if found.is_empty()
        && let Some(key) = selector
    {
        return Err(ReceiptError::Invalid {
            reason: format!("unknown local check {key:?}"),
        });
    }
    Ok(found)
}

fn record_path(state: &Path, contract: &Contract, name: &str) -> PathBuf {
    state
        .join(DIRECTORY)
        .join(&contract.name)
        .join(format!("{name}.json"))
}

/// Whether this state directory contains observations made by the local runner.
#[must_use]
#[inline]
pub fn has_local_results(state: &Path) -> bool {
    state.join(DIRECTORY).is_dir()
}

/// Inspect declared obligations and their current dependencies without executing project code.
#[inline]
pub fn inspect_local(
    root: &Path,
    state: &Path,
    selector: Option<&str>,
) -> Result<Vec<Evidence>, ReceiptError> {
    let tree = plan(root, &Selection::default())?;
    inspect_selected_local(&tree, state, selector)
}

/// Compare the current dependencies of checks declared by this plan to their saved observations.
#[inline]
pub fn inspect_selected_local(
    tree: &Plan,
    state: &Path,
    selector: Option<&str>,
) -> Result<Vec<Evidence>, ReceiptError> {
    select_checks(tree, selector)?
        .into_iter()
        .map(|(contract, name)| {
            let path = record_path(state, contract, name);
            let saved = Record::read(&path).map_err(|error| report_storage_error(&path, error))?;
            let current = Binding::read(tree, contract, name);
            Ok(report(contract, name, saved.as_ref(), &current))
        })
        .collect()
}

fn refresh_binding(
    root: &Path,
    contract: &Contract,
    name: &str,
) -> io::Result<(Binding, Snapshot)> {
    let after = plan(root, &Selection::default()).map_err(io::Error::other)?;
    read_current(&after, contract, name)
}

fn read_current(tree: &Plan, contract: &Contract, name: &str) -> io::Result<(Binding, Snapshot)> {
    let current = tree
        .contracts
        .iter()
        .find(|item| {
            item.path == contract.path
                && item.name == contract.name
                && item.obligations.iter().any(|id| id == name)
        })
        .ok_or_else(|| io::Error::other("check was removed during the run"))?;
    Binding::read(tree, current, name)
}

const fn explain_execution(is_first: bool, mode: LocalMode, can_reuse: bool) -> &'static str {
    if is_first {
        "first run"
    } else if matches!(mode, LocalMode::Fresh) {
        "fresh run requested"
    } else if !can_reuse {
        "reuse disabled in binding"
    } else {
        "retry unresolved checker"
    }
}

async fn observe_started(
    root: &Path,
    path: &Path,
    binding: &Binding,
    snapshot: &Snapshot,
) -> io::Result<Observation> {
    Record::new(
        Observation::mark_unresolved("checker started without a completed observation"),
        None,
    )
    .write(path)?;
    Ok(process::observe(root, binding, &snapshot.program)
        .await
        .unwrap_or_else(|error| Observation::mark_unresolved(error.to_string())))
}

async fn execute_one(
    tree: &Plan,
    contract: &Contract,
    name: &str,
    path: &Path,
    mode: LocalMode,
) -> io::Result<(Evidence, Option<Snapshot>)> {
    let _lock = store::lock(path)?;
    let saved = Record::read(path)?;
    let current = refresh_binding(&tree.root, contract, name);
    let mut row = report(contract, name, saved.as_ref(), &current);
    let (binding, snapshot) = match current {
        Ok(pair) => pair,
        Err(error) => {
            let observation = Observation::mark_unresolved(error.to_string());
            row.finding = None;
            row.message = Some(observation.message.clone());
            row.observed_outcome = Some(Outcome::Unresolved);
            Record::new(observation, None).write(path)?;
            return Ok((row, None));
        },
    };
    if mode == LocalMode::Reuse
        && binding.cache
        && row.freshness == Freshness::Current
        && matches!(row.observed_outcome, Some(Outcome::Pass | Outcome::Fail))
    {
        row.reused = Some(true);
        return Ok((row, Some(snapshot)));
    }
    let reason = row
        .reason
        .take()
        .unwrap_or_else(|| explain_execution(saved.is_none(), mode, binding.cache).to_owned());
    let observation = observe_started(&tree.root, path, &binding, &snapshot).await?;
    let after = refresh_binding(&tree.root, contract, name);
    let completed = match &after {
        Ok((_, latest)) if latest == &snapshot => Record::new(observation, Some(snapshot)),
        _ => {
            Record::new(
                Observation::mark_unresolved(
                    "inputs changed or became unavailable during the check",
                ),
                None,
            )
        },
    };
    completed.write(path)?;
    let mut result = report(contract, name, Some(&completed), &after);
    result.reused = Some(false);
    if result.reason.is_none() {
        result.reason = Some(reason);
    }
    Ok((result, completed.snapshot))
}

/// Run trusted, project-owned commands from their explicit bindings in declaration order.
#[inline]
pub async fn run_local(
    root: &Path,
    state: &Path,
    selector: Option<&str>,
    mode: LocalMode,
) -> Result<Vec<Evidence>, ReceiptError> {
    let tree = plan(root, &Selection::default())?;
    run_selected_local(&tree, state, selector, mode).await
}

/// Execute only the local checks declared by contracts in an already selected semantic plan.
/// Bindings still resolve their full project dependencies, even for a path-limited check.
#[inline]
pub async fn run_selected_local(
    tree: &Plan,
    state: &Path,
    selector: Option<&str>,
    mode: LocalMode,
) -> Result<Vec<Evidence>, ReceiptError> {
    let mut completed = Vec::new();
    for (contract, name) in select_checks(tree, selector)? {
        let path = record_path(state, contract, name);
        let (row, snapshot) = execute_one(tree, contract, name, &path, mode)
            .await
            .map_err(|error| report_storage_error(&path, error))?;
        completed.push((contract, name, row, snapshot));
    }
    let mut rows = Vec::new();
    let final_tree = if completed.len() > 1 {
        Some(plan(&tree.root, &Selection::default())?)
    } else {
        None
    };
    for (contract, name, mut row, snapshot) in completed {
        if let (Some(after), Some(before)) = (&final_tree, snapshot) {
            recheck_inputs(&mut row, &before, read_current(after, contract, name));
        }
        rows.push(row);
    }
    Ok(rows)
}
