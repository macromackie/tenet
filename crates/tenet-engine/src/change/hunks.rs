use core::ops::Range;

use serde::{Deserialize, Serialize};

use super::Change;

/// The inclusive old and new line ranges supplied for one complete diff hunk.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HunkRange {
    pub new: Option<[usize; 2]>,
    pub old: Option<[usize; 2]>,
}

pub(crate) struct DiffHunk {
    pub patch: String,
    pub range: HunkRange,
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "The offsets are bounded by the captured file's line count."
)]
fn convert_range(range: Range<usize>) -> Option<[usize; 2]> {
    if range.is_empty() {
        None
    } else {
        Some([range.start + 1, range.end])
    }
}

impl Change {
    pub(crate) fn diff_hunks(&self) -> Vec<DiffHunk> {
        let before = self.before.as_deref().unwrap_or("");
        let after = self.after.as_deref().unwrap_or("");
        let diff = similar::TextDiff::from_lines(before, after);
        diff.unified_diff()
            .context_radius(3)
            .iter_hunks()
            .filter_map(|hunk| {
                let first = hunk.ops().first()?;
                let last = hunk.ops().last()?;
                Some(DiffHunk {
                    patch: hunk.to_string(),
                    range: HunkRange {
                        new: convert_range(first.new_range().start..last.new_range().end),
                        old: convert_range(first.old_range().start..last.old_range().end),
                    },
                })
            })
            .collect()
    }
}
