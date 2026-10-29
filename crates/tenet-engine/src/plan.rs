use alloc::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};
use core::str::FromStr;
use std::{
    io,
    path::{Component, Path, PathBuf},
};

use ev_grep_core::MAX_FILE_BYTES;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use tenet_contracts::Contract;

use crate::{
    Change,
    Plan,
    change,
    change::SourceError,
    discovery::{self, is_allowed, is_contract},
    git,
};

/// A `PATH:START-END` request whose lines are not a range counted from 1.
#[derive(Debug, thiserror::Error)]
#[error("invalid line range in {0:?}; use PATH:START-END, counting lines from 1")]
pub struct SpanError(String);

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

/// Why `--context` cannot be supplied or sent.
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error(transparent)]
    Encode(#[from] serde_json::Error),
    #[error(transparent)]
    Excerpt(#[from] ExcerptError),
    #[error(transparent)]
    Inspect(io::Error),
    #[error("context file does not exist: {:?}", .0.display())]
    Missing(PathBuf),
    #[error("context paths must be repository source files: {:?}", .0.display())]
    NotSource(PathBuf),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(
        "supplementary context is {0:?} bytes as the model receives it; limit is \
         {MAX_FILE_BYTES:?} bytes; supply only the lines needed with --context PATH:START-END"
    )]
    TooLarge(usize),
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the derived ordering."
)]
/// A repository file, whole or one inclusive range of lines counted from 1, as `--context` and
/// `record --evidence` name it: `PATH`, `PATH:LINE`, or `PATH:START-END`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SpanRequest {
    pub path: PathBuf,
    /// Inclusive, 1-based; absent for the whole file.
    pub lines: Option<(usize, usize)>,
}

impl FromStr for SpanRequest {
    type Err = SpanError;

    #[inline]
    fn from_str(text: &str) -> Result<Self, SpanError> {
        if let Some((path, lines)) = text.rsplit_once(':')
            && !path.is_empty()
            && !lines.is_empty()
            && lines
                .chars()
                .all(|character| character.is_ascii_digit() || character == '-')
        {
            let line = |digits: &str| digits.parse::<usize>().ok().filter(|line| *line > 0);
            let (start, end) = lines.split_once('-').unwrap_or((lines, lines));
            return match (line(start), line(end)) {
                (Some(first), Some(last)) if first <= last => {
                    Ok(Self {
                        path: PathBuf::from(path),
                        lines: Some((first, last)),
                    })
                },
                _ => Err(SpanError(text.to_owned())),
            };
        }
        Ok(Self {
            path: PathBuf::from(text),
            lines: None,
        })
    }
}

impl fmt::Display for SpanRequest {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.path.display();
        match self.lines {
            None => write!(f, "{path}"),
            Some((start, end)) if start == end => write!(f, "{path}:{start}"),
            Some((start, end)) => write!(f, "{path}:{start}-{end}"),
        }
    }
}

/// Kept in the review log as the text `--context` accepts, so a plain path is a whole file.
impl Serialize for SpanRequest {
    #[inline]
    fn serialize<Writer>(&self, serializer: Writer) -> Result<Writer::Ok, Writer::Error>
    where
        Writer: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for SpanRequest {
    #[inline]
    fn deserialize<Reader>(deserializer: Reader) -> Result<Self, Reader::Error>
    where
        Reader: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(Reader::Error::custom)
    }
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
    fn new(change: &Change, mut lines: Vec<[usize; 2]>) -> Result<Self, ExcerptError> {
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

    /// Return whether the plan reads `contract`'s whole scope or only a diff of it.
    #[must_use]
    #[inline]
    pub fn find_requested_scope(&self, contract: &Contract) -> crate::RequestedScope {
        let selected_files: BTreeSet<_> = self.files.iter().collect();
        let mut scoped_candidates = self
            .candidate_files
            .iter()
            .filter(|path| self.is_governing(contract, path))
            .peekable();
        let covers_empty_scope = self.selected_paths.is_empty()
            || self
                .selected_paths
                .iter()
                .any(|selector| contract.scope.starts_with(selector));
        let covers_scope = (scoped_candidates.peek().is_some() || covers_empty_scope)
            && scoped_candidates.all(|path| selected_files.contains(path));
        if covers_scope {
            crate::RequestedScope::FullContract
        } else {
            crate::RequestedScope::SelectedSubjects
        }
    }

    /// Whether `contract` governs `path` at its current location or, for a rename, its previous
    /// one.
    #[must_use]
    #[inline]
    pub fn is_governing(&self, contract: &Contract, path: &Path) -> bool {
        discovery::is_governing(contract, &self.previous_paths, path)
    }

    /// Whether a diff changes none of the contract's files and not the contract itself, so the
    /// contract is unaffected without a model request.
    #[must_use]
    #[inline]
    pub fn is_untouched(&self, contract: &Contract) -> bool {
        self.mode == crate::Mode::Diff
            && !self
                .files
                .iter()
                .any(|path| self.is_governing(contract, path))
            && !self.contract_changes.contains(&contract.path)
    }

    /// Contract documents the diff deletes. Moving one counts too, unless discovery still finds a
    /// contract document at its new path.
    #[must_use]
    #[inline]
    pub fn list_deleted_contracts(&self) -> Vec<PathBuf> {
        let moved_to_contract = |old: &PathBuf| {
            self.previous_paths.iter().any(|(new, previous)| {
                previous == old
                    && self.contract_changes.contains(new)
                    && self.inventory.binary_search(new).is_ok()
            })
        };
        self.contract_changes
            .iter()
            .filter(|path| !self.root.join(path).exists() && !moved_to_contract(path))
            .cloned()
            .collect()
    }

    pub(crate) fn read_change(&self, path: &Path) -> Result<Change, SourceError> {
        let old = self.previous_paths.get(path);
        let before = if let Some(base) = &self.base {
            git::read_before(&self.root, base, old.map_or(path, PathBuf::as_path))?
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

    /// Supply repository files as context, each once. A file named without lines is supplied
    /// whole; otherwise only every range named for it. Lines the file no longer holds make it a
    /// capture error, so recapturing an earlier check shows its input changed; a new check or
    /// export refuses them with [`Plan::admit_context`].
    #[inline]
    pub fn with_context(mut self, requests: &[SpanRequest]) -> Result<Self, ContextError> {
        let mut ranges: BTreeMap<PathBuf, Vec<[usize; 2]>> = (self.excerpts.iter())
            .map(|(path, excerpt)| (path.clone(), excerpt.lines.clone()))
            .collect();
        for request in requests {
            let path = &request.path;
            if !is_source(&self.root, path).map_err(ContextError::Inspect)? {
                return Err(ContextError::NotSource(path.clone()));
            }
            let known = self.context.iter().any(|change| &change.path == path);
            match request.lines {
                None => {
                    drop(ranges.remove(path));
                },
                // A file already supplied whole stays whole.
                Some(_) if known && !ranges.contains_key(path) => {},
                Some((start, end)) => {
                    ranges
                        .entry(path.clone())
                        .or_default()
                        .push((start, end).into());
                },
            }
            if known {
                continue;
            }
            self.context.push(self.read_change(path)?);
        }
        self.excerpts = (self.context.iter())
            .filter_map(|change| {
                let lines = ranges.remove(&change.path)?;
                let excerpt = Excerpt::new(change, lines.clone()).unwrap_or_else(|error| {
                    Excerpt {
                        lines,
                        contents: Err(error),
                    }
                });
                Some((change.path.clone(), excerpt))
            })
            .collect();
        Ok(self)
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
