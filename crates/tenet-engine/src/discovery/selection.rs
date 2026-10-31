use alloc::collections::{BTreeMap, BTreeSet};
use core::iter;
use std::path::{Component, Path, PathBuf};

use tenet_contracts::Contract;

use super::{
    Coverage,
    DiscoveryError,
    ExcludedPath,
    Exclusion,
    Plan,
    Selection,
    is_allowed,
    is_contract,
    list_paths,
    read_contracts,
};
use crate::{git, lock};

/// Plan which contracts govern which files for the selection under `root`.
#[inline]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
pub fn plan(root: &Path, selection: &Selection) -> Result<Plan, DiscoveryError> {
    let canonical = root.canonicalize().map_err(DiscoveryError::InvalidRoot)?;
    let discovered = list_paths(&canonical)?;
    // Git reports changes inside a separate project that discovery skipped.
    let projects = lock::read_lock(&canonical)?.compile_separate_projects()?;
    let changes = selection
        .base
        .as_ref()
        .map(|reference| git::list_changed(&canonical, reference, &discovered))
        .transpose()?;
    let mut contracts = read_contracts(&canonical, &discovered, |scope| {
        selection.contract.is_some()
            || changes.as_ref().is_none_or(|listed| {
                listed.paths.iter().any(|(path, old)| {
                    path.starts_with(scope)
                        || old
                            .as_ref()
                            .is_some_and(|old_path| old_path.starts_with(scope))
                })
            })
    })?;
    if let Some(name) = &selection.contract
        && !contracts.iter().any(|contract| &contract.name == name)
    {
        return Err(DiscoveryError::UnknownContract(name.clone()));
    }
    if contracts.is_empty() && selection.base.is_none() {
        return Err(DiscoveryError::NoContracts(canonical));
    }
    let mut selectors = Vec::new();
    for path in &selection.paths {
        if !Selection::is_relative_path(path) {
            return Err(DiscoveryError::SelectedOutside);
        }
        if canonical
            .join(path)
            .symlink_metadata()
            .map_err(DiscoveryError::Metadata)?
            .file_type()
            .is_symlink()
        {
            return Err(DiscoveryError::SelectedSymbolicLink);
        }
        let full = canonical.join(path).canonicalize().map_err(|source| {
            DiscoveryError::SelectedMissing {
                path: path.clone(),
                source,
            }
        })?;
        if !full.starts_with(&canonical) {
            return Err(DiscoveryError::SelectedEscapes);
        }
        selectors.push(full.strip_prefix(&canonical)?.to_owned());
    }
    let candidates: Vec<_> = if let Some(listed) = &changes {
        listed
            .paths
            .keys()
            .filter(|path| {
                is_allowed(path) && !path.ancestors().skip(1).any(|dir| projects.is_match(dir))
            })
            .cloned()
            .collect()
    } else {
        discovered.clone()
    };
    let previous_paths: BTreeMap<_, _> = changes
        .as_ref()
        .map(|change| {
            change
                .paths
                .iter()
                .filter_map(|(path, old)| {
                    old.as_ref()
                        .map(|previous| (path.clone(), previous.clone()))
                })
                .collect()
        })
        .unwrap_or_default();
    let contract_changes: Vec<_> = changes
        .as_ref()
        .map(|listed| {
            listed
                .paths
                .iter()
                .flat_map(|(path, old)| iter::once(path).chain(old.as_ref()))
                .filter_map(|path| find_contract_document(path, &discovered))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        })
        .unwrap_or_default();
    let selects = |selector: &Path, path: &Path| {
        path.starts_with(selector)
            || previous_paths
                .get(path)
                .is_some_and(|old| old.starts_with(selector))
    };
    let selected_candidates: Vec<_> = candidates
        .iter()
        .filter(|path| {
            selectors.is_empty() || selectors.iter().any(|selector| selects(selector, path))
        })
        .collect();
    // Coverage counts every discovered contract, not only those this check runs.
    let coverage = changes.as_ref().map(|_| {
        let governed = |path: &PathBuf| {
            find_contract_document(path, &discovered).is_some()
                || contracts
                    .iter()
                    .any(|contract| is_governing(contract, &previous_paths, path))
        };
        let ungoverned_files: Vec<_> = selected_candidates
            .iter()
            .filter(|path| !is_contract(path) && !governed(path))
            .map(|path| (*path).clone())
            .collect();
        Coverage {
            changed_files: selected_candidates.len(),
            ungoverned_files,
            governed: selected_candidates.iter().any(|path| governed(path)),
        }
    });
    if let Some(name) = &selection.contract {
        contracts.retain(|contract| &contract.name == name);
    }
    if selection.contract.is_none() && !selectors.is_empty() {
        // Keep a contract when a selector covers its scope or selects a file it governs. An empty
        // directory inside its scope keeps it too, so requirements for missing files stay
        // checkable.
        contracts.retain(|contract| {
            selectors.iter().any(|selector| {
                contract.scope.starts_with(selector)
                    || (selector.starts_with(&contract.scope)
                        && canonical.join(selector).is_dir()
                        && !selected_candidates
                            .iter()
                            .any(|path| selects(selector, path)))
            }) || selected_candidates
                .iter()
                .any(|path| is_governing(contract, &previous_paths, path))
        });
    }
    let files: Vec<_> = selected_candidates
        .into_iter()
        .filter(|path| {
            contracts
                .iter()
                .any(|contract| is_governing(contract, &previous_paths, path))
        })
        .cloned()
        .collect();
    let excluded_paths = selectors
        .iter()
        .filter(|selector| !files.iter().any(|path| selects(selector, path)))
        .filter_map(|selector| {
            let reason = if candidates.iter().any(|path| selects(selector, path)) {
                Exclusion::NotGoverned
            } else if discovered.iter().any(|path| path.starts_with(selector)) {
                // A diff with no changes here.
                return None;
            } else {
                Exclusion::NotDiscovered
            };
            Some(ExcludedPath {
                path: selector.clone(),
                reason,
            })
        })
        .collect();
    let deleted = files
        .iter()
        .filter(|path| !canonical.join(path).exists())
        .cloned()
        .collect();
    let head = git::read_head(&canonical);
    let mode = if changes.is_some() {
        crate::Mode::Diff
    } else {
        crate::Mode::Full
    };
    Ok(Plan {
        root: canonical,
        contracts,
        files,
        deleted,
        base: changes.map(|listed| listed.base),
        head,
        mode,
        inventory: discovered,
        candidate_files: candidates,
        selected_paths: selectors,
        excluded_paths,
        previous_paths,
        context: Vec::new(),
        excerpts: BTreeMap::new(),
        contract_changes,
        coverage,
    })
}

/// Whether `contract` governs `path`, or the path it was renamed from.
pub(crate) fn is_governing(
    contract: &Contract,
    previous_paths: &BTreeMap<PathBuf, PathBuf>,
    path: &Path,
) -> bool {
    contract.is_governing(path)
        || previous_paths
            .get(path)
            .is_some_and(|old| contract.is_governing(old))
}

/// The contract document that `path` belongs to: itself for a `CONTRACT.md`, or the discovered
/// `CONTRACT.md` of the numbered folder that holds another file, such as an installed asset.
fn find_contract_document(path: &Path, discovered: &[PathBuf]) -> Option<PathBuf> {
    if is_contract(path) && path.file_name().is_some_and(|name| name == "CONTRACT.md") {
        return Some(path.to_owned());
    }
    let mut parts = path.components();
    let folder = parts
        .by_ref()
        .take_while(|part| part.as_os_str() != ".contracts")
        .collect::<PathBuf>()
        .join(".contracts")
        .join(parts.next()?);
    let _: Component<'_> = parts.next()?;
    let document = folder.join("CONTRACT.md");
    discovered
        .binary_search(&document)
        .is_ok()
        .then_some(document)
}
