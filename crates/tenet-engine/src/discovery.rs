use alloc::collections::{BTreeMap, BTreeSet};
use core::iter;
use std::{
    ffi::OsStr,
    fs,
    io,
    path::{Component, Path, PathBuf, StripPrefixError},
};

use ignore::WalkBuilder;
use serde::Serialize;
use tenet_contracts::{Contract, DocumentError, LockError};

use crate::{
    adoption::{self, LockFileError},
    git::{self, GitError},
};

/// Why the project's contracts or a check's files cannot be discovered.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("contract too large: {:?}", .0.display())]
    ContractTooLarge(PathBuf),
    #[error(transparent)]
    Document(#[from] DocumentError),
    #[error("duplicate contract name: {0:?}")]
    DuplicateName(String),
    #[error("duplicate contract number in {:?}", .0.display())]
    DuplicateNumber(PathBuf),
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("invalid project root")]
    InvalidRoot(#[source] io::Error),
    #[error(transparent)]
    Lock(#[from] LockError),
    #[error(transparent)]
    LockFile(#[from] LockFileError),
    #[error(transparent)]
    Metadata(io::Error),
    #[error("no contracts found under {:?}", .0.display())]
    NoContracts(PathBuf),
    #[error("contract must be a regular file: {:?}", .0.display())]
    NotRegularContract(PathBuf),
    #[error(transparent)]
    OutsideRoot(#[from] StripPrefixError),
    #[error("cannot read {:?}", .path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("selected path escapes project")]
    SelectedEscapes,
    #[error("selected path does not exist: {:?}", .path.display())]
    SelectedMissing { path: PathBuf, source: io::Error },
    #[error("selected paths must be relative and stay within the project")]
    SelectedOutside,
    #[error("selected paths must not be symbolic links")]
    SelectedSymbolicLink,
    #[error("unknown contract: {0:?}")]
    UnknownContract(String),
    #[error("file discovery failed")]
    Walk(#[source] ignore::Error),
}

impl DiscoveryError {
    /// The refused contract document, when this error is one.
    #[must_use]
    #[inline]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The remaining variants take no action here, and listing each would repeat the \
                  enum."
    )]
    pub const fn as_document(&self) -> Option<&DocumentError> {
        match self {
            Self::Document(error) => Some(error),
            _ => None,
        }
    }
}

/// The paths, contracts, and base that a command asks to check.
#[derive(Debug, Default)]
pub struct Selection {
    pub base: Option<String>,
    pub contract: Option<String>,
    pub paths: Vec<PathBuf>,
}

/// The contracts and files a check reads, and the files it leaves out.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Debug, Serialize)]
pub struct Plan {
    pub root: PathBuf,
    pub contracts: Vec<Contract>,
    pub files: Vec<PathBuf>,
    pub deleted: Vec<PathBuf>,
    pub base: Option<String>,
    pub head: Option<String>,
    pub mode: crate::Mode,
    /// Every discovered file, contract documents included; each contract governs part of it.
    pub inventory: Vec<PathBuf>,
    pub candidate_files: Vec<PathBuf>,
    pub selected_paths: Vec<PathBuf>,
    /// Selected paths that yield no subject for any planned contract.
    pub excluded_paths: Vec<ExcludedPath>,
    pub previous_paths: BTreeMap<PathBuf, PathBuf>,
    pub context: Vec<crate::Change>,
    /// `context` files supplied as line ranges instead of whole; the rest are supplied whole.
    pub excerpts: BTreeMap<PathBuf, crate::Excerpt>,
    /// The contract documents a diff changes, directly or through another file in their folder,
    /// such as an installed asset.
    pub contract_changes: Vec<PathBuf>,
    /// How much of a diff's selected change the contracts govern; `None` for a full check.
    pub coverage: Option<Coverage>,
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
/// How much of a diff's selected change any contract governs, whichever contracts a check runs.
#[derive(Clone, Debug, Serialize)]
pub struct Coverage {
    /// Selected changed files, contract documents included.
    pub changed_files: usize,
    /// Changed files no contract governs at their current or previous path. Files in a
    /// `.contracts` folder are contract changes, not ungoverned files.
    pub ungoverned_files: Vec<PathBuf>,
    /// Whether a contract governs any changed file or the change edits a contract's folder. It
    /// is false when no contract applies, including when nothing changed.
    pub governed: bool,
}

/// A selected path where a check assesses nothing, and why.
#[derive(Clone, Debug, Serialize)]
pub struct ExcludedPath {
    pub path: PathBuf,
    pub reason: Exclusion,
}

/// Why discovery left a file out of a check.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Exclusion {
    /// Discovery finds no file there: ignore rules, a protected name, or an empty directory.
    NotDiscovered,
    /// Discovery finds files there, but no planned contract governs them.
    NotGoverned,
}

/// Names never discovered at any depth: Git internals, review state, the adoption lock, and
/// environment files that may hold credentials.
///
/// Nested repositories and worktrees, which carry their own `.git`, are separate projects, as are
/// the directories the lock's `projects` names. Ignore files exclude everything else.
/// The names a scan never enters or reads.
const PROTECTED_NAMES: [&str; 4] = [".git", ".tenet", ".tenet.lock", ".env"];

fn is_protected(name: &OsStr) -> bool {
    let text = name.to_string_lossy();
    PROTECTED_NAMES.contains(&text.as_ref()) || text.starts_with(".env.")
}

pub(crate) fn list_paths(root: &Path) -> Result<Vec<PathBuf>, DiscoveryError> {
    let projects = adoption::read_lock(root)?.compile_separate_projects()?;
    let prefix = root.to_owned();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            entry.depth() == 0
                || !(is_protected(entry.file_name())
                    || entry.file_type().is_some_and(|kind| kind.is_dir())
                        && (entry.path().join(".git").symlink_metadata().is_ok()
                            || entry
                                .path()
                                .strip_prefix(&prefix)
                                .is_ok_and(|path| projects.is_match(path))))
        })
        .build();
    let mut paths = Vec::new();
    for entry in walker {
        let dir_entry = entry.map_err(DiscoveryError::Walk)?;
        if dir_entry.file_type().is_some_and(|kind| !kind.is_dir()) {
            paths.push(dir_entry.path().strip_prefix(root)?.to_owned());
        }
    }
    paths.sort();
    Ok(paths)
}

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
fn read_contracts(
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
    let projects = adoption::read_lock(&canonical)?.compile_separate_projects()?;
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
        if !path.components().all(|component| {
            match component {
                Component::Normal(_) | Component::CurDir => true,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir => false,
            }
        }) {
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

pub(crate) fn is_contract(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == ".contracts")
}

pub(crate) fn is_allowed(path: &Path) -> bool {
    !path.components().any(|part| is_protected(part.as_os_str()))
}
