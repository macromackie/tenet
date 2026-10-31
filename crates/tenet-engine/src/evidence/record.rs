use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::hash_content;

/// Whether a captured file was selected for the check or read as support.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Selected,
    Support,
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
/// A captured file's identity. Contents live in `.tenet/blobs/` under these hashes.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct CapturedFile {
    pub path: PathBuf,
    pub previous_path: Option<PathBuf>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub error: Option<String>,
    /// In a diff, the resulting lines the change wrote, as in `Change::changed`, so a record can
    /// be held to the change. Input keys leave them out; `before` and `after` determine them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<[usize; 2]>,
    /// For a file named with `--context PATH:START-END`, the resulting lines the check supplied,
    /// so the input key holds them and a record cites a supporting file only within them. Empty
    /// for a whole file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excerpt: Vec<[usize; 2]>,
}

impl CapturedFile {
    /// The citations that show this file's change: each changed range, or the cleared file.
    #[must_use]
    #[inline]
    pub fn change_citations(&self) -> Vec<String> {
        let path = self.path.display();
        if self.is_cleared() {
            return vec![format!("{path} (whole file)")];
        }
        (self.changed.iter())
            .map(|[start, end]| {
                if start == end {
                    format!("{path}:{start}")
                } else {
                    format!("{path}:{start}-{end}")
                }
            })
            .collect()
    }

    /// Whether the change touched this file: wrote lines in it or deleted it.
    #[must_use]
    #[inline]
    pub const fn is_changed(&self) -> bool {
        !self.changed.is_empty() || self.is_deleted()
    }

    /// Whether a citation of this file, with `lines` or whole, overlaps or adjoins a line the
    /// change wrote. Only a deleted or emptied file is cited whole.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Line numbers are bounded by the file's line count."
    )]
    #[must_use]
    #[inline]
    pub fn is_citing_change(&self, lines: Option<[usize; 2]>) -> bool {
        match lines {
            None => self.is_cleared(),
            Some([start, end]) => {
                (self.changed.iter()).any(|&[first, last]| start <= last + 1 && first <= end + 1)
            },
        }
    }

    /// Whether the change left this file without text, by deleting or emptying it, so no line
    /// shows the change.
    fn is_cleared(&self) -> bool {
        self.before.is_some()
            && (self.after.as_deref()).is_none_or(|after| after == hash_content(""))
    }

    /// Return whether the change deleted the file.
    #[must_use]
    #[inline]
    pub const fn is_deleted(&self) -> bool {
        self.before.is_some() && self.after.is_none()
    }
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
/// A captured file in one contract's scope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopedFile {
    pub role: Role,
    #[serde(flatten)]
    pub file: CapturedFile,
}
