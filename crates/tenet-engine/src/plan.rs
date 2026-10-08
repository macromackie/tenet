use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

use anyhow::{Result, ensure};

use crate::{
    Plan,
    discovery::{allowed, is_contract},
    git,
};

impl Plan {
    pub fn file_input(
        &self,
        contract: &tenet_contracts::Contract,
        change: &crate::Change,
    ) -> Result<serde_json::Value> {
        let context: Vec<_> = self
            .context
            .iter()
            .filter(|support| support.path != change.path)
            .map(crate::Change::current_input)
            .collect();
        let state = serde_json::json!({
            "contract": contract.body,
            "change": change,
            "files": [change.current_input()],
            "context": context,
        });
        crate::change::check_size("complete file input", serde_json::to_vec(&state)?.len())?;
        Ok(state)
    }

    pub fn with_context(mut self, paths: &[PathBuf]) -> Result<Self> {
        let mut seen: BTreeSet<_> = self
            .context
            .iter()
            .map(|change| change.path.clone())
            .collect();
        for path in paths {
            ensure!(
                path.components()
                    .all(|part| matches!(part, Component::Normal(_)))
                    && allowed(path)
                    && !is_contract(path),
                "context paths must be repository source files: {}",
                path.display()
            );
            if !seen.insert(path.clone()) {
                continue;
            }
            let full = self.root.join(path);
            if full.exists() {
                ensure!(
                    full.canonicalize()?.starts_with(&self.root),
                    "context path escapes repository"
                );
            }
            let change = self.change(path)?;
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

    pub(crate) fn in_scope(&self, path: &Path, scope: &Path) -> bool {
        path.starts_with(scope)
            || self
                .previous_paths
                .get(path)
                .is_some_and(|old| old.starts_with(scope))
    }

    pub fn requested_scope(&self, contract: &tenet_contracts::Contract) -> crate::RequestedScope {
        let mut scoped_candidates = self
            .candidate_files
            .iter()
            .filter(|path| self.in_scope(path, &contract.scope))
            .peekable();
        let covers_empty_scope = self.selected_paths.is_empty()
            || self
                .selected_paths
                .iter()
                .any(|selector| contract.scope.starts_with(selector));
        let covers_scope = (scoped_candidates.peek().is_some() || covers_empty_scope)
            && scoped_candidates.all(|path| self.files.contains(path));
        if covers_scope {
            crate::RequestedScope::FullContract
        } else {
            crate::RequestedScope::SelectedSubjects
        }
    }

    pub fn change(&self, path: &Path) -> Result<crate::Change> {
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
