use std::path::{Path, PathBuf};

use anyhow::Result;
use ev_grep_core::{MAX_FILE_BYTES, Source, SourceRead, SourceTooLarge};
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
    /// Sent once, as the resulting file in model inputs.
    #[serde(skip_serializing)]
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
        let text = serde_json::to_string(&serde_json::json!({
            "change": self,
            "file": self.current_input(),
        }))?;
        check_size("before/after input", text.len())?;
        Ok(Source {
            path: self.path.display().to_string(),
            text,
            focus: None,
        })
    }

    pub(crate) fn current_input(&self) -> serde_json::Value {
        serde_json::json!({
            "path": self.path,
            "contents": self.after,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct InputLimit {
    label: &'static str,
    bytes: u64,
}

impl std::fmt::Display for InputLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} is {} bytes; limit is {} bytes; input was not truncated",
            self.label, self.bytes, MAX_FILE_BYTES
        )
    }
}

impl std::error::Error for InputLimit {}

pub(crate) fn check_size(label: &'static str, bytes: usize) -> Result<()> {
    if bytes > MAX_FILE_BYTES {
        return Err(InputLimit {
            label,
            bytes: bytes as u64,
        }
        .into());
    }
    Ok(())
}

pub(crate) fn text(root: &Path, path: &Path) -> Result<String> {
    let source = ev_grep_core::read_source(&root.join(path)).map_err(|error| {
        if let Some(limit) = error.downcast_ref::<SourceTooLarge>() {
            InputLimit {
                label: "source file",
                bytes: limit.bytes,
            }
            .into()
        } else {
            error
        }
    })?;
    match source {
        SourceRead::Text(source) => Ok(source.text),
        SourceRead::Binary => anyhow::bail!(
            "binary input cannot establish contract compliance: {}",
            path.display()
        ),
    }
}
