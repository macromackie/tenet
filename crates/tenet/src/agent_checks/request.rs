use alloc::collections::BTreeMap;
use std::{
    io,
    path::{Path, absolute},
};

use serde_json::{Value, json};
use tenet_contracts::Contract;
use tenet_engine::{Plan, review::Checked};

use super::{Finding, Operation};

pub(super) fn dispatch(
    plan: &Plan,
    state: &Path,
    scopes: &BTreeMap<&str, &Contract>,
    operation: &Operation<'_>,
) -> io::Result<Vec<Finding>> {
    let (max_generations, checked, known) = match operation {
        Operation::Inspect | Operation::Replay => (0, None, &[][..]),
        Operation::Assess {
            max_generations,
            checked,
            known,
            ..
        } => (*max_generations, Some(*checked), *known),
    };
    let contracts: Vec<_> = scopes
        .values()
        .map(|contract| build_contract_input(plan, contract, checked, known))
        .collect();
    let input = json!({
        "root": plan.root,
        "base": plan.base,
        "state": absolute(state)?,
        "contracts": contracts,
        "inspect": matches!(operation, Operation::Inspect),
        "fresh": matches!(operation, Operation::Assess { fresh: true, .. }),
        "maxGenerations": max_generations,
    });

    super::process::execute(&input, &plan.root, max_generations > 0)
}
pub(super) fn build_contract_input(
    plan: &Plan,
    contract: &Contract,
    checked: Option<&Checked>,
    known: &[Finding],
) -> Value {
    let paths: Vec<_> = plan
        .inventory
        .iter()
        .filter(|path| contract.is_governing(path))
        .collect();
    let folder = contract.path.parent().unwrap_or_else(|| Path::new("."));
    let sections = ["assets", "checks", "resources"].map(|name| folder.join(name));
    // The full inventory honors ignore rules, separate projects and protected paths.
    // The semantic export exposes only governed sources, so it cannot supply reference material.
    let hints: Vec<_> = plan
        .inventory
        .iter()
        .filter(|path| sections.iter().any(|section| path.starts_with(section)))
        .take(128)
        .collect();
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
                .map(|result| (&result.path, result.status))
                .collect::<BTreeMap<_, _>>()
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
