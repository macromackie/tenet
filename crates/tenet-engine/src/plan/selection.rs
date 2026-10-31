use alloc::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tenet_contracts::Contract;

use crate::{Plan, discovery};

impl Plan {
    /// Return whether the plan reads `contract`'s whole scope or only a diff of it.
    #[must_use]
    #[inline]
    pub fn find_requested_scope(&self, contract: &Contract) -> crate::RequestedScope {
        let selected_files: BTreeSet<_> = self.files.iter().collect();
        let mut scoped_candidates = self
            .candidate_files
            .iter()
            .filter(|path| self.is_governing(contract, path))
            .peekable();
        let covers_empty_scope = self.selected_paths.is_empty()
            || self
                .selected_paths
                .iter()
                .any(|selector| contract.scope.starts_with(selector));
        let covers_scope = (scoped_candidates.peek().is_some() || covers_empty_scope)
            && scoped_candidates.all(|path| selected_files.contains(path));
        if covers_scope {
            crate::RequestedScope::FullContract
        } else {
            crate::RequestedScope::SelectedSubjects
        }
    }

    /// Whether `contract` governs `path` at its current location or, for a rename, its previous
    /// one.
    #[must_use]
    #[inline]
    pub fn is_governing(&self, contract: &Contract, path: &Path) -> bool {
        discovery::is_governing(contract, &self.previous_paths, path)
    }

    /// Whether a diff changes none of the contract's files and not the contract itself, so the
    /// contract is unaffected without a model request.
    #[must_use]
    #[inline]
    pub fn is_untouched(&self, contract: &Contract) -> bool {
        self.mode == crate::Mode::Diff
            && !self
                .files
                .iter()
                .any(|path| self.is_governing(contract, path))
            && !self.contract_changes.contains(&contract.path)
    }

    /// Contract documents the diff deletes. Moving one counts too, unless discovery still finds a
    /// contract document at its new path.
    #[must_use]
    #[inline]
    pub fn list_deleted_contracts(&self) -> Vec<PathBuf> {
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
}
