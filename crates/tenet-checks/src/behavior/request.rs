use alloc::collections::BTreeMap;
use std::{
    io,
    path::{Path, PathBuf, absolute},
};

use serde_json::{Value, json};
use tenet_contracts::Contract;
use tenet_engine::{Plan, Status, review::Checked};

use super::{Finding, Operation};

pub(super) fn dispatch(
    plan: &Plan,
    state: &Path,
    scopes: &BTreeMap<&str, &Contract>,
    operation: &Operation<'_>,
    on_progress: Option<&mut dyn FnMut(&str)>,
) -> io::Result<Vec<Finding>> {
    let (max_generations, checked, stored_decisions, known, is_fresh) = match operation {
        Operation::Inspect { decisions } => (0, None, *decisions, &[][..], false),
        Operation::Assess {
            max_generations,
            checked,
            known,
            fresh,
        } => (*max_generations, Some(*checked), None, *known, *fresh),
    };
    let contracts: Vec<_> = scopes
        .values()
        .map(|contract| build_contract_input(plan, contract, checked, stored_decisions, known))
        .collect();
    let input = json!({
        "root": absolute(&plan.root)?,
        "base": plan.base,
        "state": absolute(state)?,
        "contracts": contracts,
        "inspect": matches!(operation, Operation::Inspect { .. }),
        "fresh": is_fresh,
        "maxGenerations": max_generations,
    });

    super::process::execute(&input, &plan.root, max_generations > 0, on_progress)
}
fn collect_contract_hints<'plan>(plan: &'plan Plan, contract: &Contract) -> Vec<&'plan PathBuf> {
    let folder = contract.path.parent().unwrap_or_else(|| Path::new("."));
    let sections =
        ["assets", "checks", "resources", super::pins::SCRIPTS_DIR].map(|name| folder.join(name));
    let lock_file = folder.join(super::pins::SCRIPTS_LOCK_FILE);
    // The full inventory honors ignore rules, separate projects and protected paths.
    // The semantic export exposes only governed sources, so it cannot supply reference material.
    plan.inventory
        .iter()
        .filter(|path| {
            path.as_path() != lock_file.as_path()
                && path.file_name().is_none_or(|name| name != ".lock.json")
                && sections.iter().any(|section| path.starts_with(section))
        })
        .take(128)
        .collect()
}

pub(super) fn build_contract_input(
    plan: &Plan,
    contract: &Contract,
    checked: Option<&Checked>,
    stored_decisions: Option<&BTreeMap<String, Status>>,
    known: &[Finding],
) -> Value {
    let paths: Vec<_> = plan
        .inventory
        .iter()
        .filter(|path| contract.is_governing(path))
        .collect();
    let hints = collect_contract_hints(plan, contract);
    let decisions = checked
        .and_then(|report| {
            report
                .contracts
                .iter()
                .find(|entry| entry.contract == contract.name)
        })
        .map(|entry| {
            entry
                .results
                .iter()
                .map(|result| (result.path.as_str(), result.status))
                .collect::<BTreeMap<_, _>>()
        })
        .or_else(|| {
            stored_decisions.map(|saved| {
                saved
                    .iter()
                    .map(|(path, status)| (path.as_str(), *status))
                    .collect::<BTreeMap<_, _>>()
            })
        });
    let mechanical: Vec<_> = known
        .iter()
        .filter(|row| row.contract == contract.name)
        .collect();
    json!({
        "document": contract.document,
        "assets": hints,
        "mechanical": mechanical,
        "decisions": decisions,
        "checks": contract.obligations,
        "hash": contract.hash,
        "name": contract.name,
        "paths": paths,
        "path": contract.path,
    })
}
