use std::{
    collections::{BTreeMap, BTreeSet},
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
    pub partial: bool,
    pub mode: crate::Mode,
    pub inventory: Vec<PathBuf>,
    pub previous_paths: BTreeMap<PathBuf, PathBuf>,
    pub context: Vec<crate::Change>,
    pub contract_changes: Vec<PathBuf>,
}

pub(crate) fn paths(root: &Path) -> Result<Vec<PathBuf>> {
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            entry.depth() == 0
                || (!matches!(
                    name.as_ref(),
                    ".git"
                        | ".context"
                        | ".agents"
                        | ".tenet"
                        | ".env"
                        | "target"
                        | "node_modules"
                        | "dist"
                        | "out"
                ) && !name.starts_with(".env."))
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
    let mut contracts = Vec::new();
    for path in paths(root)? {
        if path.file_name().is_none_or(|n| n != "CONTRACT.md")
            || !path.components().any(|p| p.as_os_str() == ".contracts")
        {
            continue;
        }
        let full = root.join(&path);
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
            &path,
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
    let mut contracts = discover_contracts(&root)?;
    if let Some(name) = &selection.contract {
        contracts.retain(|c| &c.name == name);
        ensure!(!contracts.is_empty(), "unknown contract: {name}");
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
    let changes = selection
        .base
        .as_ref()
        .map(|r| git::changed(&root, r))
        .transpose()?;
    let inventory: Vec<_> = paths(&root)?
        .into_iter()
        .filter(|p| !is_contract(p))
        .collect();
    let candidates: Vec<_> = if let Some(changes) = &changes {
        changes
            .paths
            .keys()
            .filter(|p| !is_contract(p) && allowed(p))
            .cloned()
            .collect()
    } else {
        inventory.clone()
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
    if selection.contract.is_none()
        && let Some(changes) = &changes
    {
        contracts.retain(|contract| {
            changes.paths.iter().any(|(path, old)| {
                path.starts_with(&contract.scope)
                    || old
                        .as_ref()
                        .is_some_and(|path| path.starts_with(&contract.scope))
            })
        });
    }
    let files: Vec<_> = candidates
        .into_iter()
        .filter(|p| {
            let old = previous_paths.get(p);
            (selectors.is_empty()
                || selectors
                    .iter()
                    .any(|s| p.starts_with(s) || old.is_some_and(|p| p.starts_with(s))))
                && contracts.iter().any(|c| {
                    p.starts_with(&c.scope) || old.is_some_and(|p| p.starts_with(&c.scope))
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
        inventory,
        previous_paths,
        context: Vec::new(),
        contract_changes,
        base: changes.map(|c| c.base),
        partial: !selectors.is_empty() && !selectors.iter().any(|p| p.as_os_str().is_empty()),
    })
}

pub(crate) fn is_contract(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == ".contracts")
}

pub(crate) fn allowed(path: &Path) -> bool {
    !path.components().any(|part| {
        let name = part.as_os_str().to_string_lossy();
        matches!(
            name.as_ref(),
            ".git"
                | ".context"
                | ".agents"
                | ".tenet"
                | ".env"
                | "target"
                | "node_modules"
                | "dist"
                | "out"
        ) || name.starts_with(".env.")
    })
}
