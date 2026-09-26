use std::path::{Path, PathBuf};

use anyhow::{Result, ensure};
use ev_grep_core::{MAX_FILE_BYTES, Source, SourceRead};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Full,
    Diff,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Clone, Debug, Serialize)]
pub struct Change {
    pub path: PathBuf,
    pub previous_path: Option<PathBuf>,
    pub kind: ChangeKind,
    pub patch: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

impl Change {
    pub fn new(path: PathBuf, before: Option<String>, after: Option<String>) -> Self {
        let kind = match (&before, &after) {
            (None, _) => ChangeKind::Added,
            (_, None) => ChangeKind::Deleted,
            _ => ChangeKind::Modified,
        };
        let patch = match (&before, &after) {
            (Some(before), Some(after)) => similar::TextDiff::from_lines(before, after)
                .unified_diff()
                .context_radius(3)
                .to_string(),
            _ => String::new(),
        };
        Self {
            path,
            patch,
            previous_path: None,
            kind,
            before,
            after,
        }
    }

    pub(crate) fn source(&self) -> Result<Source> {
        let text = serde_json::to_string(self)?;
        ensure!(
            text.len() <= MAX_FILE_BYTES,
            "before/after input exceeds 64 KiB; input was not truncated"
        );
        Ok(Source {
            path: self.path.display().to_string(),
            text,
            focus: None,
        })
    }
}

pub(crate) fn text(root: &Path, path: &Path) -> Result<String> {
    match ev_grep_core::read_source(&root.join(path))? {
        SourceRead::Text(source) => Ok(source.text),
        SourceRead::Binary => anyhow::bail!(
            "binary input cannot establish contract compliance: {}",
            path.display()
        ),
    }
}
