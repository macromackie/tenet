use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use ignore::WalkBuilder;
use serde::Serialize;
use tenet_contracts::Contract;

use crate::git;

#[derive(Default)]
pub struct Selection {
    pub paths: Vec<PathBuf>,
    pub base: Option<String>,
    pub contract: Option<String>,
}

#[derive(Serialize)]
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
    pub contract_changes: Vec<PathBuf>,
    /// How much of a diff's selected change the contracts govern; `None` for a full check.
    pub coverage: Option<Coverage>,
}

/// How much of a diff's selected change any contract governs, whichever contracts a check runs.
#[derive(Clone, Debug, Serialize)]
pub struct Coverage {
    /// Selected changed files, contract documents included.
    pub changed_files: usize,
    /// Changed files no contract governs at their current or previous path. Files in a
    /// `.contracts` folder are contract changes, not ungoverned files.
    pub ungoverned_files: Vec<PathBuf>,
    /// Whether a contract governs any changed file or the change edits a contract document. It
    /// is false when no contract applies, including when nothing changed.
    pub governed: bool,
}

/// A selected path where a check assesses nothing, and why.
#[derive(Clone, Debug, Serialize)]
pub struct ExcludedPath {
    pub path: PathBuf,
    pub reason: Exclusion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Exclusion {
    /// Discovery finds no file there: ignore rules, a protected name, or an empty directory.
    NotDiscovered,
    /// Discovery finds files there, but no planned contract governs them.
    NotGoverned,
}

/// Names never discovered at any depth: Git internals, review state, the adoption lock, and
/// environment files that may hold credentials. Nested repositories and worktrees, which carry
/// their own `.git`, are separate projects. Ignore files exclude everything else.
fn protected(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    matches!(name.as_ref(), ".git" | ".tenet" | ".tenet.lock" | ".env") || name.starts_with(".env.")
}

pub(crate) fn paths(root: &Path) -> Result<Vec<PathBuf>> {
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|entry| {
            entry.depth() == 0
                || !(protected(entry.file_name())
                    || entry.file_type().is_some_and(|t| t.is_dir())
                        && entry.path().join(".git").symlink_metadata().is_ok())
        })
        .build();
    let mut paths = Vec::new();
    for entry in walker {
        let entry = entry.context("file discovery failed")?;
        if entry.file_type().is_some_and(|t| !t.is_dir()) {
            paths.push(entry.path().strip_prefix(root)?.to_owned());
        }
    }
    paths.sort();
    Ok(paths)
}

pub fn discover_contracts(root: &Path) -> Result<Vec<Contract>> {
    read_contracts(root, &paths(root)?, |_| true)
}

fn read_contracts(
    root: &Path,
    paths: &[PathBuf],
    include_scope: impl Fn(&Path) -> bool,
) -> Result<Vec<Contract>> {
    let mut contracts = Vec::new();
    for path in paths {
        if path.file_name().is_none_or(|n| n != "CONTRACT.md")
            || !path.components().any(|p| p.as_os_str() == ".contracts")
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
        ensure!(
            full.symlink_metadata()?.is_file(),
            "contract must be a regular file: {}",
            path.display()
        );
        ensure!(
            full.metadata()?.len() <= 65536,
            "contract too large: {}",
            path.display()
        );
        contracts.push(tenet_contracts::parse(
            path,
            &fs::read_to_string(&full)
                .with_context(|| format!("cannot read {}", path.display()))?,
        )?);
    }
    let mut names = BTreeSet::new();
    let mut numbers = BTreeSet::new();
    for contract in &contracts {
        ensure!(
            names.insert(&contract.name),
            "duplicate contract name: {}",
            contract.name
        );
        ensure!(
            numbers.insert((&contract.scope, contract.number)),
            "duplicate contract number in {}",
            contract.scope.display()
        );
    }
    contracts.sort_by(|a, b| {
        (a.scope.components().count(), &a.scope, a.number).cmp(&(
            b.scope.components().count(),
            &b.scope,
            b.number,
        ))
    });
    Ok(contracts)
}

pub fn plan(root: &Path, selection: &Selection) -> Result<Plan> {
    let root = root.canonicalize().context("invalid project root")?;
    let discovered = paths(&root)?;
    let changes = selection
        .base
        .as_ref()
        .map(|r| git::changed(&root, r, &discovered))
        .transpose()?;
    let mut contracts = read_contracts(&root, &discovered, |scope| {
        selection.contract.is_some()
            || changes.as_ref().is_none_or(|changes| {
                changes.paths.iter().any(|(path, old)| {
                    path.starts_with(scope)
                        || old.as_ref().is_some_and(|path| path.starts_with(scope))
                })
            })
    })?;
    if let Some(name) = &selection.contract {
        ensure!(
            contracts.iter().any(|c| &c.name == name),
            "unknown contract: {name}"
        );
    }
    ensure!(
        !contracts.is_empty() || selection.base.is_some(),
        "no contracts found under {}",
        root.display()
    );
    let mut selectors = Vec::new();
    for path in &selection.paths {
        ensure!(
            path.components()
                .all(|c| matches!(c, Component::Normal(_) | Component::CurDir)),
            "selected paths must be relative and stay within the project"
        );
        ensure!(
            !root.join(path).symlink_metadata()?.file_type().is_symlink(),
            "selected paths must not be symbolic links"
        );
        let full = root
            .join(path)
            .canonicalize()
            .with_context(|| format!("selected path does not exist: {}", path.display()))?;
        ensure!(full.starts_with(&root), "selected path escapes project");
        selectors.push(full.strip_prefix(&root)?.to_owned());
    }
    let candidates: Vec<_> = if let Some(changes) = &changes {
        changes
            .paths
            .keys()
            .filter(|p| allowed(p))
            .cloned()
            .collect()
    } else {
        discovered.clone()
    };
    let previous_paths: BTreeMap<_, _> = changes
        .as_ref()
        .map(|c| {
            c.paths
                .iter()
                .filter_map(|(p, old)| old.as_ref().map(|old| (p.clone(), old.clone())))
                .collect()
        })
        .unwrap_or_default();
    let contract_changes: Vec<_> = changes
        .as_ref()
        .map(|changes| {
            changes
                .paths
                .iter()
                .flat_map(|(path, old)| std::iter::once(path).chain(old.as_ref()))
                .filter(|path| {
                    is_contract(path) && path.file_name().is_some_and(|name| name == "CONTRACT.md")
                })
                .cloned()
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
        .filter(|p| selectors.is_empty() || selectors.iter().any(|s| selects(s, p)))
        .collect();
    // Coverage counts every discovered contract, not only those this check runs.
    let coverage = changes.as_ref().map(|_| {
        let governed = |path: &PathBuf| {
            contract_changes.contains(path)
                || contracts
                    .iter()
                    .any(|contract| governs(contract, &previous_paths, path))
        };
        let ungoverned_files: Vec<_> = selected_candidates
            .iter()
            .filter(|path| !is_contract(path) && !governed(path))
            .map(|path| (*path).clone())
            .collect();
        Coverage {
            changed_files: selected_candidates.len(),
            governed: selected_candidates.iter().any(|path| governed(path)),
            ungoverned_files,
        }
    });
    if let Some(name) = &selection.contract {
        contracts.retain(|c| &c.name == name);
    }
    if selection.contract.is_none() && !selectors.is_empty() {
        // Keep a contract when a selector covers its scope or selects a file it governs. An empty
        // directory inside its scope keeps it too, so requirements for missing files stay checkable.
        contracts.retain(|contract| {
            selectors.iter().any(|selector| {
                contract.scope.starts_with(selector)
                    || (selector.starts_with(&contract.scope)
                        && root.join(selector).is_dir()
                        && !selected_candidates
                            .iter()
                            .any(|path| selects(selector, path)))
            }) || selected_candidates
                .iter()
                .any(|path| governs(contract, &previous_paths, path))
        });
    }
    let files: Vec<_> = selected_candidates
        .into_iter()
        .filter(|path| {
            contracts
                .iter()
                .any(|contract| governs(contract, &previous_paths, path))
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
        .filter(|p| !root.join(p).exists())
        .cloned()
        .collect();
    let head = git::head(&root);
    let mode = if changes.is_some() {
        crate::Mode::Diff
    } else {
        crate::Mode::Full
    };
    Ok(Plan {
        root,
        contracts,
        files,
        deleted,
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
        base: changes.map(|c| c.base),
    })
}

/// Whether `contract` governs `path`, or the path it was renamed from.
pub(crate) fn governs(
    contract: &Contract,
    previous_paths: &BTreeMap<PathBuf, PathBuf>,
    path: &Path,
) -> bool {
    contract.governs(path)
        || previous_paths
            .get(path)
            .is_some_and(|old| contract.governs(old))
}

pub(crate) fn is_contract(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == ".contracts")
}

pub(crate) fn allowed(path: &Path) -> bool {
    !path.components().any(|part| protected(part.as_os_str()))
}
