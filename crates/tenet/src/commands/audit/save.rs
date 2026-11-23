use alloc::collections::BTreeSet;
use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use serde::Serialize;
use tenet_checks::behavior::{AuditModuleReport, Outcome, PinBundleState, copy_staged_pin_bundle};
use tenet_engine::discover_contracts;

use crate::error::CommandError;

const CONTRACTS_DIR: &str = ".contracts";
const VERIFIED_OUTCOMES: &[Outcome] = &[Outcome::Failed, Outcome::Passed];

/// One contract saved into `.contracts/` or `.tenet/scan/.contracts/`.
#[derive(Clone, Debug, Serialize)]
pub(in crate::commands) struct SavedContract {
    pub name: String,
    pub path: PathBuf,
    pub pin_saved: bool,
}

/// Collect contract names discovered across every scope under `root`.
pub(in crate::commands) fn read_global_contract_names(
    root: &Path,
) -> Result<BTreeSet<String>, CommandError> {
    let contracts = discover_contracts(root)?;
    Ok(contracts.into_iter().map(|item| item.name).collect())
}

/// Write synthesized contracts that produced verified checks into `contracts_dir`,
/// deduplicating against `existing_names` and existing folders in `contracts_dir`.
pub(in crate::commands) fn save_contracts_to(
    contracts_dir: &Path,
    existing_names: &BTreeSet<String>,
    state: &Path,
    reports: &[AuditModuleReport],
) -> Result<Vec<SavedContract>, CommandError> {
    let mut seen_names = existing_names.clone();
    seen_names.extend(read_existing_names(contracts_dir));
    let mut next_number = read_next_number(contracts_dir);
    let mut saved = Vec::new();
    for report in reports {
        let verified_names = list_verified_contract_names(report);
        for contract in &report.contracts {
            if !verified_names.contains(&contract.name) || seen_names.contains(&contract.name) {
                continue;
            }
            let folder_name = format!("{next_number:03}-{}", contract.name);
            let folder = contracts_dir.join(&folder_name);
            fs::create_dir_all(&folder)
                .with_context(|| format!("create contract folder {:?}", folder.display()))?;
            let doc_path = folder.join("CONTRACT.md");
            fs::write(&doc_path, &contract.document)
                .with_context(|| format!("write contract {:?}", doc_path.display()))?;
            let pin_saved =
                copy_staged_pin_bundle(&state.join(&contract.lock_path), &folder).with_context(
                    || format!("materialize behavior scripts in {:?}", folder.display()),
                )? == PinBundleState::Written;
            let _inserted = seen_names.insert(contract.name.clone());
            next_number = next_number.saturating_add(1);
            saved.push(SavedContract {
                name: contract.name.clone(),
                path: doc_path,
                pin_saved,
            });
        }
    }
    Ok(saved)
}

/// Write synthesized contracts that produced verified executable checks into `<root>/.contracts/`.
pub(super) fn save_verified_contracts(
    root: &Path,
    state: &Path,
    reports: &[AuditModuleReport],
) -> Result<Vec<SavedContract>, CommandError> {
    let contracts_root = root.join(CONTRACTS_DIR);
    let existing = read_global_contract_names(root)?;
    let mut saved = save_contracts_to(&contracts_root, &existing, state, reports)?;
    strip_saved_path_prefix(&mut saved, root);
    Ok(saved)
}

/// Strip `base` from saved contract paths where possible for concise display and JSON output.
pub(in crate::commands) fn strip_saved_path_prefix(saved: &mut [SavedContract], base: &Path) {
    for item in saved {
        if let Ok(relative) = item.path.strip_prefix(base) {
            item.path = relative.to_path_buf();
        }
    }
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
