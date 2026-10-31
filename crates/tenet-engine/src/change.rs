mod base;
mod excerpt;

use alloc::string::FromUtf8Error;
use core::num::ParseIntError;
use std::{
    fs,
    io,
    path::{Path, PathBuf, StripPrefixError},
};

pub(crate) use base::read_before;
use ev_grep_core::MAX_FILE_BYTES;
pub use excerpt::{Excerpt, ExcerptError, format_line_ranges};
use serde::{Deserialize, Serialize};
use similar::DiffOp;

use crate::git::GitError;

/// The most bytes a check captures from the files it reads.
pub const MAX_CAPTURE_BYTES: usize = 64 * 1024 * 1024;

/// Whether a check reads whole files or a diff against a base.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Diff,
    Full,
}

impl Mode {
    /// Return the mode's name as reports spell it.
    #[must_use]
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Diff => "diff",
        }
    }
}

/// How a change touches a file: added, deleted, modified, or renamed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Deleted,
    Modified,
    Renamed,
}

/// One file's contents before and after a change.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
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
    pub(crate) fn describe_current_input(&self) -> serde_json::Value {
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

    /// Build a change from a file's contents before and after it, deriving its kind.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Similar's hunk offsets and lengths fall inside the file's line count."
    )]
    #[must_use]
    #[inline]
    pub fn new(path: PathBuf, before: Option<String>, after: Option<String>) -> Self {
        let kind = match (&before, &after) {
            (None, _) => ChangeKind::Added,
            (_, None) => ChangeKind::Deleted,
            _ => ChangeKind::Modified,
        };
        let (unified, changed) = match (&before, &after) {
            (Some(old_text), Some(new_text)) => {
                let diff = similar::TextDiff::from_lines(old_text, new_text);
                let changed = diff
                    .ops()
                    .iter()
                    .filter_map(|op| {
                        match *op {
                            DiffOp::Equal { .. } => None,
                            DiffOp::Delete { new_index, .. } => Some([new_index.max(1); 2]),
                            DiffOp::Insert {
                                new_index, new_len, ..
                            }
                            | DiffOp::Replace {
                                new_index, new_len, ..
                            } => Some([new_index + 1, new_index + new_len]),
                        }
                    })
                    .collect();
                let unified_diff = diff.unified_diff().context_radius(3).to_string();
                (unified_diff, changed)
            },
            (None, Some(new_text)) if !new_text.is_empty() => {
                (String::new(), vec![[1, new_text.lines().count()]])
            },
            _ => (String::new(), Vec::new()),
        };
        Self {
            path,
            previous_path: None,
            kind,
            patch: unified,
            before,
            after,
            changed,
        }
    }
}

/// Stands in for text left out of a model input; the reason says why.
pub(crate) fn describe_omitted(reason: &str) -> serde_json::Value {
    serde_json::json!({ "omitted": reason })
}

/// Source with no text a model can judge. The file stays in scope and in the inventory, but its
/// file check is skipped. Only an empty file's contents reach the combined assessment.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Unreadable {
    #[error("binary file")]
    Binary,
    #[error("empty file")]
    Empty,
    #[error("not UTF-8 text")]
    NotUtf8,
    #[error("symbolic link")]
    SymbolicLink,
}

/// Why a file's source, current or at the base commit, cannot be captured.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("base input must be a regular file: {0:?}")]
    BaseNotRegularFile(String),
    #[error(transparent)]
    BaseSize(#[from] ParseIntError),
    #[error(transparent)]
    BaseSizeText(#[from] FromUtf8Error),
    #[error(
        "{:?} is {bytes:?} bytes; capture limit is {MAX_CAPTURE_BYTES:?} bytes",
        .path.display()
    )]
    CaptureLimit { path: PathBuf, bytes: u64 },
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("cannot inspect {:?}", .path.display())]
    Inspect { path: PathBuf, source: io::Error },
    #[error("expected a regular file: {:?}", .0.display())]
    NotRegularFile(PathBuf),
    #[error("path is not UTF-8")]
    NotUtf8Path,
    #[error(transparent)]
    OutsideRepository(#[from] StripPrefixError),
    #[error("cannot read {:?}", .path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error(transparent)]
    Unreadable(#[from] Unreadable),
}

#[expect(
    clippy::map_err_ignore,
    reason = "An unreadable source is reported by kind only, without the UTF-8 error offset."
)]
pub(crate) fn decode(bytes: Vec<u8>) -> Result<String, Unreadable> {
    if bytes.contains(&0) {
        return Err(Unreadable::Binary);
    }
    String::from_utf8(bytes).map_err(|_| Unreadable::NotUtf8)
}

#[expect(
    clippy::as_conversions,
    reason = "A usize fits in a u64 on every supported target."
)]
pub(crate) fn check_capture_size(path: &Path, bytes: u64) -> Result<(), SourceError> {
    if bytes > MAX_CAPTURE_BYTES as u64 {
        return Err(SourceError::CaptureLimit {
            path: path.to_owned(),
            bytes,
        });
    }
    Ok(())
}

/// A file a check left out because it exceeds the input size limit.
#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("{label:?} is {bytes:?} bytes; limit is {MAX_FILE_BYTES:?} bytes; input was not truncated")]
pub struct InputLimit {
    bytes: u64,
    label: &'static str,
}

#[expect(
    clippy::as_conversions,
    reason = "A usize fits in a u64 on every supported target."
)]
pub(crate) const fn check_size(label: &'static str, bytes: usize) -> Result<(), InputLimit> {
    if bytes > MAX_FILE_BYTES {
        return Err(InputLimit {
            bytes: bytes as u64,
            label,
        });
    }
    Ok(())
}

pub(crate) fn read_text(root: &Path, path: &Path) -> Result<String, SourceError> {
    let full = root.join(path);
    let metadata = full.symlink_metadata().map_err(|source| {
        SourceError::Inspect {
            path: path.to_owned(),
            source,
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(Unreadable::SymbolicLink.into());
    }
    if !metadata.is_file() {
        return Err(SourceError::NotRegularFile(path.to_owned()));
    }
    check_capture_size(path, metadata.len())?;
    let bytes = fs::read(&full).map_err(|source| {
        SourceError::Read {
            path: path.to_owned(),
            source,
        }
    })?;
    Ok(decode(bytes)?)
}
