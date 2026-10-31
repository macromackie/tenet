use alloc::collections::BTreeSet;
use std::{
    fs,
    path::{Path, PathBuf},
};

use tenet_contracts::Contract;

use super::{DiscoveryError, list_paths};

/// Find every contract under `root`, in scope and numeric order.
#[inline]
pub fn discover_contracts(root: &Path) -> Result<Vec<Contract>, DiscoveryError> {
    read_contracts(root, &list_paths(root)?, |_| true)
}

#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
pub(super) fn read_contracts(
    root: &Path,
    paths: &[PathBuf],
    include_scope: impl Fn(&Path) -> bool,
) -> Result<Vec<Contract>, DiscoveryError> {
    let mut contracts = Vec::new();
    for path in paths {
        if path.file_name().is_none_or(|name| name != "CONTRACT.md")
            || !path
                .components()
                .any(|component| component.as_os_str() == ".contracts")
        {
            continue;
        }
        let scope: PathBuf = path
            .iter()
            .take_while(|part| *part != ".contracts")
            .collect();
        if !include_scope(&scope) {
            continue;
        }
        let full = root.join(path);
        if !full
            .symlink_metadata()
            .map_err(DiscoveryError::Metadata)?
            .is_file()
        {
            return Err(DiscoveryError::NotRegularContract(path.clone()));
        }
        if full.metadata().map_err(DiscoveryError::Metadata)?.len() > 0x0001_0000 {
            return Err(DiscoveryError::ContractTooLarge(path.clone()));
        }
        let document = fs::read_to_string(&full).map_err(|source| {
            DiscoveryError::Read {
                path: path.clone(),
                source,
            }
        })?;
        contracts.push(tenet_contracts::parse(path, &document)?);
    }
    let mut names = BTreeSet::new();
    let mut numbers = BTreeSet::new();
    for contract in &contracts {
        if !names.insert(&contract.name) {
            return Err(DiscoveryError::DuplicateName(contract.name.clone()));
        }
        if !numbers.insert((&contract.scope, contract.number)) {
            return Err(DiscoveryError::DuplicateNumber(contract.scope.clone()));
        }
    }
    contracts.sort_by(|left, right| {
        (left.scope.components().count(), &left.scope, left.number).cmp(&(
            right.scope.components().count(),
            &right.scope,
            right.number,
        ))
    });
    Ok(contracts)
}
