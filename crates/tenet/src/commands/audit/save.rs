use alloc::collections::BTreeSet;
use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use serde::Serialize;
use tenet_checks::behavior::{AuditModuleReport, Outcome};

use crate::error::CommandError;

const CONTRACTS_DIR: &str = ".contracts";
const VERIFIED_OUTCOMES: &[Outcome] = &[Outcome::Failed, Outcome::Passed];

/// One contract saved into `.contracts/` when `tenet audit --save` runs.
#[derive(Clone, Debug, Serialize)]
pub(super) struct SavedContract {
    pub name: String,
    pub path: PathBuf,
    pub pin_saved: bool,
}

/// Write synthesized contracts that produced verified executable checks into `.contracts/`.
pub(super) fn save_verified_contracts(
    root: &Path,
    state: &Path,
    reports: &[AuditModuleReport],
) -> Result<Vec<SavedContract>, CommandError> {
    let contracts_root = root.join(CONTRACTS_DIR);
    let mut existing_names = read_existing_names(&contracts_root);
    let mut next_number = read_next_number(&contracts_root);
    let mut saved = Vec::new();
    for report in reports {
        let verified_names = list_verified_contract_names(report);
        for contract in &report.contracts {
            if !verified_names.contains(&contract.name) || existing_names.contains(&contract.name) {
                continue;
            }
            let folder_name = format!("{next_number:03}-{}", contract.name);
            let folder = contracts_root.join(&folder_name);
            fs::create_dir_all(&folder)
                .with_context(|| format!("create contract folder {:?}", folder.display()))?;
            let doc_path = folder.join("CONTRACT.md");
            fs::write(&doc_path, &contract.document)
                .with_context(|| format!("write contract {:?}", doc_path.display()))?;
            let pin_saved = copy_staged_pin(state, &contract.lock_path, &folder)?.is_some();
            let relative = PathBuf::from(CONTRACTS_DIR)
                .join(&folder_name)
                .join("CONTRACT.md");
            let _inserted = existing_names.insert(contract.name.clone());
            next_number = next_number.saturating_add(1);
            saved.push(SavedContract {
                name: contract.name.clone(),
                path: relative,
                pin_saved,
            });
        }
    }
    Ok(saved)
}

fn copy_staged_pin(
    state: &Path,
    lock_path: &str,
    folder: &Path,
) -> Result<Option<PathBuf>, CommandError> {
    let source = state.join(lock_path);
    if !source.is_file() {
        return Ok(None);
    }
    let destination = folder.join("agent-checks.lock.json");
    let _copied = fs::copy(&source, &destination)
        .with_context(|| format!("copy staged pin to {:?}", destination.display()))?;
    Ok(Some(destination))
}

fn list_verified_contract_names(report: &AuditModuleReport) -> BTreeSet<String> {
    report
        .findings
        .iter()
        .filter(|row| VERIFIED_OUTCOMES.contains(&row.outcome))
        .map(|row| row.contract.clone())
        .collect()
}

fn read_existing_names(contracts_root: &Path) -> BTreeSet<String> {
    let Ok(entries) = fs::read_dir(contracts_root) else {
        return BTreeSet::new();
    };
    let mut names = BTreeSet::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let text = name.to_string_lossy();
        let stripped = text
            .split_once('-')
            .map_or_else(|| text.as_ref(), |(_, rest)| rest);
        let _inserted = names.insert(stripped.to_owned());
    }
    names
}

fn read_next_number(contracts_root: &Path) -> u32 {
    let Ok(entries) = fs::read_dir(contracts_root) else {
        return 1;
    };
    let mut highest: u32 = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if let Some((prefix, _)) = text.split_once('-')
            && let Ok(number) = prefix.parse::<u32>()
        {
            highest = highest.max(number);
        }
    }
    highest.saturating_add(1)
}
