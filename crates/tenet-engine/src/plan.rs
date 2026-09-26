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
    pub fn with_context(mut self, paths: &[PathBuf]) -> Result<Self> {
        let mut seen = BTreeSet::new();
        for path in paths {
            ensure!(
                path.components()
                    .all(|part| matches!(part, Component::Normal(_)))
                    && allowed(path)
                    && !is_contract(path),
                "context paths must be repository source files: {}",
                path.display()
            );
            if !seen.insert(path) {
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

    pub(crate) fn change(&self, path: &Path) -> Result<crate::Change> {
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
