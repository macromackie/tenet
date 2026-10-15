use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, ensure};
use ev_grep_core::MAX_FILE_BYTES;
use serde::{Deserialize, Serialize};
use similar::DiffOp;

use crate::MAX_CAPTURE_BYTES;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Full,
    Diff,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Diff => "diff",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
    /// The resulting lines the change wrote, as inclusive 1-based ranges: every line of an added
    /// file, none of a deleted one. Lines removed from a kept file count as the line before them,
    /// or line 1. Derived from `before` and `after`, so it never reaches a model input.
    #[serde(skip)]
    pub changed: Vec<[usize; 2]>,
}

impl Change {
    pub fn new(path: PathBuf, before: Option<String>, after: Option<String>) -> Self {
        let kind = match (&before, &after) {
            (None, _) => ChangeKind::Added,
            (_, None) => ChangeKind::Deleted,
            _ => ChangeKind::Modified,
        };
        let (patch, changed) = match (&before, &after) {
            (Some(before), Some(after)) => {
                let diff = similar::TextDiff::from_lines(before, after);
                let changed = diff
                    .ops()
                    .iter()
                    .filter_map(|op| match *op {
                        DiffOp::Equal { .. } => None,
                        DiffOp::Delete { new_index, .. } => Some([new_index.max(1); 2]),
                        DiffOp::Insert {
                            new_index, new_len, ..
                        }
                        | DiffOp::Replace {
                            new_index, new_len, ..
                        } => Some([new_index + 1, new_index + new_len]),
                    })
                    .collect();
                let patch = diff.unified_diff().context_radius(3).to_string();
                (patch, changed)
            }
            (None, Some(after)) if !after.is_empty() => {
                (String::new(), vec![[1, after.lines().count()]])
            }
            _ => (String::new(), Vec::new()),
        };
        Self {
            path,
            patch,
            previous_path: None,
            kind,
            before,
            after,
            changed,
        }
    }

    pub(crate) fn current_input(&self) -> serde_json::Value {
        serde_json::json!({
            "path": self.path,
            "contents": self.after,
        })
    }

    /// Whether the change has no text to judge: an empty file added, kept, or removed.
    pub(crate) fn is_empty(&self) -> bool {
        [&self.before, &self.after]
            .into_iter()
            .all(|text| text.as_deref().is_none_or(str::is_empty))
    }
}

/// Stands in for text left out of a model input; the reason says why.
pub(crate) fn omitted(reason: &str) -> serde_json::Value {
    serde_json::json!({ "omitted": reason })
}

/// Source with no text a model can judge. The file stays in scope and in the inventory, but its
/// file check is skipped. Only an empty file's contents reach the combined assessment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Unreadable {
    Empty,
    Binary,
    SymbolicLink,
    NotUtf8,
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Empty => "empty file",
            Self::Binary => "binary file",
            Self::SymbolicLink => "symbolic link",
            Self::NotUtf8 => "not UTF-8 text",
        })
    }
}

impl std::error::Error for Unreadable {}

pub(crate) fn decode(bytes: Vec<u8>) -> Result<String> {
    if bytes.contains(&0) {
        return Err(Unreadable::Binary.into());
    }
    String::from_utf8(bytes).map_err(|_| Unreadable::NotUtf8.into())
}

pub(crate) fn check_capture_size(path: &Path, bytes: u64) -> Result<()> {
    ensure!(
        bytes <= MAX_CAPTURE_BYTES as u64,
        "{} is {bytes} bytes; capture limit is {MAX_CAPTURE_BYTES} bytes",
        path.display()
    );
    Ok(())
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
    let full = root.join(path);
    let metadata = full
        .symlink_metadata()
        .with_context(|| format!("cannot inspect {}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(Unreadable::SymbolicLink.into());
    }
    ensure!(
        metadata.is_file(),
        "expected a regular file: {}",
        path.display()
    );
    check_capture_size(path, metadata.len())?;
    decode(std::fs::read(&full).with_context(|| format!("cannot read {}", path.display()))?)
}
