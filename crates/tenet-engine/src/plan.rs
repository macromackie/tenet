use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

use anyhow::{Result, ensure};
use tenet_contracts::Contract;

use crate::{
    Plan,
    discovery::{self, allowed, is_contract},
    git,
};

impl Plan {
    pub fn with_context(mut self, paths: &[PathBuf]) -> Result<Self> {
        let mut seen: BTreeSet<_> = self
            .context
            .iter()
            .map(|change| change.path.clone())
            .collect();
        for path in paths {
            ensure!(
                is_source(&self.root, path)?,
                "context paths must be repository source files: {}",
                path.display()
            );
            if !seen.insert(path.clone()) {
                continue;
            }
            let change = self.read_change(path)?;
            ensure!(
                change.before.is_some() || change.after.is_some(),
                "context file does not exist: {}",
                path.display()
            );
            self.context.push(change);
        }
        ensure!(
            serde_json::to_vec(&self.context)?.len() <= ev_grep_core::MAX_FILE_BYTES,
            "supplementary context exceeds 64 KiB"
        );
        Ok(self)
    }

    /// Whether `contract` governs `path` at its current location or, for a rename, its previous one.
    pub fn governs(&self, contract: &Contract, path: &Path) -> bool {
        discovery::governs(contract, &self.previous_paths, path)
    }

    /// Contract documents the diff deletes. Moving one counts too, unless discovery still finds a
    /// contract document at its new path.
    pub fn deleted_contracts(&self) -> Vec<PathBuf> {
        let moved_to_contract = |old: &PathBuf| {
            self.previous_paths.iter().any(|(new, previous)| {
                previous == old
                    && self.contract_changes.contains(new)
                    && self.inventory.binary_search(new).is_ok()
            })
        };
        self.contract_changes
            .iter()
            .filter(|path| !self.root.join(path).exists() && !moved_to_contract(path))
            .cloned()
            .collect()
    }

    /// Whether a diff changes none of the contract's files and not the contract itself, so the
    /// contract is unaffected without a model request.
    pub fn untouched(&self, contract: &Contract) -> bool {
        self.mode == crate::Mode::Diff
            && !self.files.iter().any(|path| self.governs(contract, path))
            && !self.contract_changes.contains(&contract.path)
    }

    pub fn requested_scope(&self, contract: &Contract) -> crate::RequestedScope {
        let selected: BTreeSet<_> = self.files.iter().collect();
        let mut scoped_candidates = self
            .candidate_files
            .iter()
            .filter(|path| self.governs(contract, path))
            .peekable();
        let covers_empty_scope = self.selected_paths.is_empty()
            || self
                .selected_paths
                .iter()
                .any(|selector| contract.scope.starts_with(selector));
        let covers_scope = (scoped_candidates.peek().is_some() || covers_empty_scope)
            && scoped_candidates.all(|path| selected.contains(path));
        if covers_scope {
            crate::RequestedScope::FullContract
        } else {
            crate::RequestedScope::SelectedSubjects
        }
    }

    pub(crate) fn read_change(&self, path: &Path) -> Result<crate::Change> {
        let old = self.previous_paths.get(path);
        let before = if let Some(base) = &self.base {
            git::before(&self.root, base, old.map_or(path, PathBuf::as_path))?
        } else {
            None
        };
        let after = if self.root.join(path).symlink_metadata().is_ok() {
            Some(crate::change::text(&self.root, path)?)
        } else {
            None
        };
        let mut change = crate::Change::new(path.to_owned(), before, after);
        if let Some(old) = old {
            change.previous_path = Some(old.clone());
            change.kind = crate::ChangeKind::Renamed;
        }
        Ok(change)
    }
}

/// Whether a caller may supply this path as repository source, as `--context` or as a record's
/// evidence: relative, outside excluded and contract folders, and not leading out of the canonical
/// `root` through a symbolic link.
pub(crate) fn is_source(root: &Path, path: &Path) -> Result<bool> {
    let relative = path
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
        && allowed(path)
        && !is_contract(path);
    let full = root.join(path);
    Ok(relative && (!full.exists() || full.canonicalize()?.starts_with(root)))
}
