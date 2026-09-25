use std::{
    collections::BTreeSet,
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
    pub changed_since: Option<String>,
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
    pub broadened: bool,
}

fn paths(root: &Path) -> Result<Vec<PathBuf>> {
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            entry.depth() == 0
                || ((!name.starts_with('.') || name == ".contracts")
                    && !matches!(name.as_ref(), "target" | "node_modules" | "dist" | "out"))
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
        !contracts.is_empty(),
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
        .changed_since
        .as_ref()
        .map(|r| git::changed(&root, r))
        .transpose()?;
    let broadened = changes.as_ref().is_some_and(|c| c.broaden);
    let files: Vec<_> = paths(&root)?
        .into_iter()
        .filter(|p| {
            !p.components().any(|c| c.as_os_str() == ".contracts")
                && (broadened || selectors.is_empty() || selectors.iter().any(|s| p.starts_with(s)))
                && changes
                    .as_ref()
                    .is_none_or(|c| broadened || c.paths.contains(p))
                && contracts.iter().any(|c| p.starts_with(&c.scope))
        })
        .collect();
    let deleted = changes
        .as_ref()
        .map(|c| {
            c.paths
                .iter()
                .filter(|p| !root.join(p).exists())
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let head = git::head(&root);
    Ok(Plan {
        root,
        contracts,
        files,
        deleted,
        head,
        base: changes.map(|c| c.base),
        broadened,
    })
}
