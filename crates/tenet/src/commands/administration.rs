use std::{io::Write as _, path::Path};

use anyhow::{Context as _, anyhow};
use tenet_engine::{
    Selection,
    adoption::{self, AddRequest, Destination, Kind, SkillDestination, SkillRequest},
};

use crate::{
    adoption::{execute as execute_adoption, report_added},
    cli::{Contracts, ExitStatus, Skills},
    error::CommandError,
    output,
};
/// The schema version of the object that `contracts list --json` prints.
const LIST_VERSION: u64 = 1;

pub(super) fn execute_skills(root: &Path, operation: &Skills) -> Result<ExitStatus, CommandError> {
    match operation {
        Skills::Add {
            spec,
            dir,
            name,
            path: existing_path,
            sources,
        } => {
            let destination = match existing_path {
                Some(path) => SkillDestination::Existing(path),
                None => {
                    SkillDestination::Directory {
                        directory: dir,
                        name: name.as_deref(),
                    }
                },
            };
            let added = adoption::add_skill(
                root,
                &SkillRequest {
                    spec,
                    destination,
                    sources,
                },
            )?;
            report_added(&added, "tenet validate")
        },
        Skills::Locked(command) => execute_adoption(root, Kind::Skills, command),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
pub(super) fn execute_contracts(
    root: &Path,
    operation: &Contracts,
) -> Result<ExitStatus, CommandError> {
    match operation {
        Contracts::Add {
            spec,
            scope: selected_scope,
            path: existing_path,
            name,
            number,
            sources,
        } => {
            let destination = match (selected_scope, existing_path) {
                (_, Some(path)) => Destination::Existing(path),
                (Some(scope), None) => {
                    Destination::Scope {
                        scope,
                        name: name.as_deref(),
                        number: *number,
                    }
                },
                (None, None) => {
                    return Err(anyhow!("add requires --scope or --path").into());
                },
            };
            let added = adoption::add(
                root,
                &AddRequest {
                    spec,
                    destination,
                    sources,
                },
            )?;
            return report_added(&added, &format!("tenet check --contract {}", added.name));
        },
        Contracts::Locked(command) => {
            return execute_adoption(root, Kind::Contracts, command);
        },
        Contracts::List {
            path: selected_path,
            base: selected_base,
            json,
        } => {
            if let Some(path) = selected_path
                && !Selection::is_relative_path(path)
            {
                return Err(anyhow!("--for path must be relative to the project root").into());
            }
            let (contracts, contract_changes, coverage) = if let Some(base) = selected_base {
                let plan = tenet_engine::plan(
                    root,
                    &Selection {
                        base: Some(base.clone()),
                        ..Selection::default()
                    },
                )?;
                (plan.contracts, plan.contract_changes, plan.coverage)
            } else {
                (tenet_engine::discover_contracts(root)?, Vec::new(), None)
            };
            let selected: Vec<_> = contracts
                .iter()
                .filter(|contract| {
                    selected_path
                        .as_ref()
                        .is_none_or(|path| contract.is_governing(path))
                })
                .collect();
            if *json {
                let entries: Vec<_> = selected
                    .iter()
                    .map(|contract| {
                        serde_json::json!({
                            "name": contract.name,
                            "path": contract.path,
                            "scope": contract.scope,
                            "message": contract.message,
                        })
                    })
                    .collect();
                output::write_document(
                    &mut output::open_stdout(),
                    LIST_VERSION,
                    serde_json::json!({
                        "contracts": entries,
                        "contract_changes": contract_changes,
                        "coverage": coverage,
                    }),
                )?;
            } else {
                for contract in selected {
                    writeln!(
                        output::open_stdout(),
                        "{}  {}",
                        contract.name,
                        contract.path.display()
                    )?;
                }
            }
        },
        Contracts::Show { name } => {
            let contracts = tenet_engine::discover_contracts(root)?;
            let contract = contracts
                .iter()
                .find(|contract| &contract.name == name)
                .context("unknown contract")?;
            writeln!(
                output::open_stdout(),
                "{}\n\n{}",
                contract.path.display(),
                contract.document
            )?;
        },
    }
    Ok(ExitStatus::Passed)
}
