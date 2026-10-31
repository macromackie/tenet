use std::{
    io,
    path::{Component, Path, PathBuf},
};

use ev_grep_core::MAX_FILE_BYTES;

use super::ContextError;
use crate::{
    Change,
    Plan,
    change,
    change::{ExcerptError, SourceError},
    discovery::{is_allowed, is_contract},
};

impl Plan {
    /// Refuse context that a new check or export cannot send: a file that does not exist, lines
    /// the file does not hold, or more than the file input limit, as the model receives it, all
    /// together.
    #[inline]
    pub fn admit_context(&self) -> Result<(), ContextError> {
        let mut inputs = Vec::new();
        for change in &self.context {
            if change.before.is_none() && change.after.is_none() {
                return Err(ContextError::Missing(change.path.clone()));
            }
            inputs.push(self.build_context_input(change)?);
        }
        let bytes = serde_json::to_vec(&inputs)?.len();
        if bytes > MAX_FILE_BYTES {
            return Err(ContextError::TooLarge(bytes));
        }
        Ok(())
    }

    /// What the model receives for a `--context` file: its path and resulting contents, or only
    /// the lines of an excerpt.
    pub(crate) fn build_context_input(
        &self,
        change: &Change,
    ) -> Result<serde_json::Value, ExcerptError> {
        Ok(match self.excerpts.get(&change.path) {
            Some(excerpt) => {
                serde_json::json!({
                    "path": change.path,
                    "contents": excerpt.contents()?,
                })
            },
            None => change.describe_current_input(),
        })
    }

    pub(crate) fn read_change(&self, path: &Path) -> Result<Change, SourceError> {
        let old = self.previous_paths.get(path);
        let before = if let Some(base) = &self.base {
            change::read_before(&self.root, base, old.map_or(path, PathBuf::as_path))?
        } else {
            None
        };
        let after = if self.root.join(path).symlink_metadata().is_ok() {
            Some(change::read_text(&self.root, path)?)
        } else {
            None
        };
        let mut change = Change::new(path.to_owned(), before, after);
        if let Some(previous) = old {
            change.previous_path = Some(previous.clone());
            change.kind = crate::ChangeKind::Renamed;
        }
        Ok(change)
    }
}

/// Whether a caller may supply this path as repository source.
///
/// A path given as `--context` or as a record's evidence must be relative, outside excluded and
/// contract folders, and must not lead out of the canonical `root` through a symbolic link.
pub(crate) fn is_source(root: &Path, path: &Path) -> io::Result<bool> {
    let relative = path
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
        && is_allowed(path)
        && !is_contract(path);
    let full = root.join(path);
    Ok(relative && (!full.exists() || full.canonicalize()?.starts_with(root)))
}
