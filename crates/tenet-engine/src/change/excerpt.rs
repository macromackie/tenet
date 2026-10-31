use std::path::PathBuf;

use serde::Serialize;

use super::Change;

/// Why a `--context` file cannot supply the lines named for it.
#[derive(Clone, Debug, thiserror::Error)]
pub enum ExcerptError {
    #[error("cannot supply lines of {:?}: the change deletes it", .0.display())]
    Deleted(PathBuf),
    #[error("{:?}:{start:?}-{end:?} is not within the file's {lines:?} lines", .path.display())]
    OutsideFile {
        path: PathBuf,
        start: usize,
        end: usize,
        lines: usize,
    },
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
/// Lines of a `--context` file supplied in place of its whole resulting contents. A selected
/// file's lines also stand in for its resulting contents wherever they are left out for size.
#[derive(Clone, Debug, Serialize)]
#[expect(
    clippy::partial_pub_fields,
    reason = "The private fields hold state that only this crate may set."
)]
pub struct Excerpt {
    /// Inclusive and 1-based, in order, with overlapping and adjacent ranges merged.
    pub lines: Vec<[usize; 2]>,
    /// What the model receives as the file's contents: a note naming the lines, and those lines,
    /// each prefixed with its number so that citations keep the file's numbering. An error when
    /// the file's current contents do not hold the lines.
    #[serde(skip)]
    contents: Result<serde_json::Value, ExcerptError>,
}

impl Excerpt {
    /// What the model receives as the file's contents, or why the file cannot supply the lines.
    pub(crate) fn contents(&self) -> Result<serde_json::Value, ExcerptError> {
        self.contents.clone()
    }

    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "Line numbers come from the file's own lines, so ranges start at 1 and end \
                  within the file."
    )]
    pub(crate) fn new(change: &Change, mut lines: Vec<[usize; 2]>) -> Result<Self, ExcerptError> {
        let text =
            (change.after.as_deref()).ok_or_else(|| ExcerptError::Deleted(change.path.clone()))?;
        let all: Vec<&str> = text.split_inclusive('\n').collect();
        lines.sort_unstable();
        let mut merged: Vec<[usize; 2]> = Vec::new();
        for [start, end] in lines {
            if !(1 <= start && start <= end && end <= all.len()) {
                return Err(ExcerptError::OutsideFile {
                    path: change.path.clone(),
                    start,
                    end,
                    lines: all.len(),
                });
            }
            match merged.last_mut() {
                Some(last) if start <= last[1] + 1 => last[1] = last[1].max(end),
                _ => merged.push([start, end]),
            }
        }
        let mut numbered = String::new();
        for &[start, end] in &merged {
            for (number, line) in (start..=end).zip(&all[start - 1..end]) {
                numbered.push_str(&number.to_string());
                numbered.push('\t');
                numbered.push_str(line);
            }
        }
        let contents = serde_json::json!({
            "excerpt": format!(
                "Only lines {} of {} were supplied. Each line starts with its line number and \
                 a tab.",
                format_line_ranges(&merged),
                all.len()
            ),
            "lines": numbered,
        });
        Ok(Self {
            lines: merged,
            contents: Ok(contents),
        })
    }

    /// Retain the requested lines when their captured source cannot supply them.
    pub(crate) const fn new_unavailable(lines: Vec<[usize; 2]>, error: ExcerptError) -> Self {
        Self {
            lines,
            contents: Err(error),
        }
    }
}

/// Inclusive line ranges as text, such as `3, 10-12`.
#[must_use]
#[inline]
pub fn format_line_ranges(lines: &[[usize; 2]]) -> String {
    let ranges: Vec<_> = (lines.iter())
        .map(|&[start, end]| {
            if start == end {
                start.to_string()
            } else {
                format!("{start}-{end}")
            }
        })
        .collect();
    ranges.join(", ")
}
