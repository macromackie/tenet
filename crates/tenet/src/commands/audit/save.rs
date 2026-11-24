mod consolidate;

use alloc::collections::{BTreeMap, BTreeSet};
use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use consolidate::{save_consolidated_group, strip_module_shard};
use serde::Serialize;
use tenet_checks::behavior::{
    AuditContractOutput,
    AuditModuleReport,
    Outcome,
    PinBundleState,
    copy_staged_pin_bundle,
};
use tenet_engine::discover_contracts;

use crate::error::CommandError;

const CONTRACTS_DIR: &str = ".contracts";
const VERIFIED_OUTCOMES: &[Outcome] = &[Outcome::Failed, Outcome::Passed];

/// Mutable cursor tracking target directory, next contract number, and seen contract names.
struct SaveCursor<'cursor> {
    contracts_dir: &'cursor Path,
    next_number: u32,
    seen_names: BTreeSet<String>,
    state: &'cursor Path,
}

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

/// Write verified synthesized contracts into `contracts_dir`, consolidating multi-contract modules.
pub(in crate::commands) fn save_contracts_to(
    contracts_dir: &Path,
    existing_names: &BTreeSet<String>,
    state: &Path,
    reports: &[AuditModuleReport],
) -> Result<Vec<SavedContract>, CommandError> {
    let mut seen_names = existing_names.clone();
    seen_names.extend(read_existing_names(contracts_dir));
    let mut cursor = SaveCursor {
        contracts_dir,
        next_number: read_next_number(contracts_dir),
        seen_names,
        state,
    };
    let groups = group_verified_contracts(reports);
    let mut saved = Vec::new();
    for (base_module, contracts) in groups {
        match contracts.as_slice() {
            [] => {},
            [single] => {
                if let Some(entry) = save_single_contract(&mut cursor, single)? {
                    saved.push(entry);
                }
            },
            many => {
                saved.push(save_consolidated_group(&mut cursor, &base_module, many)?);
            },
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

fn group_verified_contracts(
    reports: &[AuditModuleReport],
) -> Vec<(String, Vec<&AuditContractOutput>)> {
    let mut module_indices: BTreeMap<String, usize> = BTreeMap::new();
    let mut groups: Vec<(String, Vec<&AuditContractOutput>)> = Vec::new();
    for report in reports {
        let verified_names = list_verified_contract_names(report);
        let base_module = strip_module_shard(&report.name).to_owned();
        for contract in &report.contracts {
            if !verified_names.contains(&contract.name) {
                continue;
            }
            if let Some(&index) = module_indices.get(&base_module)
                && let Some((_, list)) = groups.get_mut(index)
            {
                list.push(contract);
            } else {
                let index = groups.len();
                let _prior = module_indices.insert(base_module.clone(), index);
                groups.push((base_module.clone(), vec![contract]));
            }
        }
    }
    groups
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

fn save_single_contract(
    cursor: &mut SaveCursor<'_>,
    contract: &AuditContractOutput,
) -> Result<Option<SavedContract>, CommandError> {
    if cursor.seen_names.contains(&contract.name) {
        return Ok(None);
    }
    let source_lock = cursor.state.join(&contract.lock_path);
    let saved = write_contract_folder(
        cursor,
        contract.name.clone(),
        &contract.document,
        |folder| {
            let state = copy_staged_pin_bundle(&source_lock, folder).with_context(|| {
                format!("materialize behavior scripts in {:?}", folder.display())
            })?;
            Ok(state == PinBundleState::Written)
        },
    )?;
    Ok(Some(saved))
}

fn write_contract_folder(
    cursor: &mut SaveCursor<'_>,
    name: String,
    document: &str,
    write_pins: impl FnOnce(&Path) -> Result<bool, CommandError>,
) -> Result<SavedContract, CommandError> {
    let folder_name = format!("{:03}-{name}", cursor.next_number);
    let folder = cursor.contracts_dir.join(&folder_name);
    fs::create_dir_all(&folder)
        .with_context(|| format!("create contract folder {:?}", folder.display()))?;
    let doc_path = folder.join("CONTRACT.md");
    fs::write(&doc_path, document)
        .with_context(|| format!("write contract {:?}", doc_path.display()))?;
    let pin_saved = write_pins(&folder)?;
    let _inserted = cursor.seen_names.insert(name.clone());
    cursor.next_number = cursor.next_number.saturating_add(1);
    Ok(SavedContract {
        name,
        path: doc_path,
        pin_saved,
    })
}
