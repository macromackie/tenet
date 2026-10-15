use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context as _, Result, bail, ensure};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use tenet_contracts::Contract;

use crate::{
    Change, Plan,
    discovery::{self, allowed, is_contract},
    git,
};

/// A repository file, whole or one inclusive range of lines counted from 1, as `--context` and
/// `record --evidence` name it: `PATH`, `PATH:LINE`, or `PATH:START-END`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SpanRequest {
    pub path: PathBuf,
    /// Inclusive, 1-based; absent for the whole file.
    pub lines: Option<(usize, usize)>,
}

impl FromStr for SpanRequest {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        if let Some((path, lines)) = text.rsplit_once(':')
            && !path.is_empty()
            && !lines.is_empty()
            && lines.chars().all(|c| c.is_ascii_digit() || c == '-')
        {
            let line = |text: &str| text.parse::<usize>().ok().filter(|line| *line > 0);
            let (start, end) = lines.split_once('-').unwrap_or((lines, lines));
            return match (line(start), line(end)) {
                (Some(start), Some(end)) if start <= end => Ok(Self {
                    path: PathBuf::from(path),
                    lines: Some((start, end)),
                }),
                _ => {
                    bail!("invalid line range in {text}; use PATH:START-END, counting lines from 1")
                }
            };
        }
        Ok(Self {
            path: PathBuf::from(text),
            lines: None,
        })
    }
}

impl fmt::Display for SpanRequest {
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
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for SpanRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

/// Lines of a `--context` file supplied in place of its whole resulting contents. A selected
/// file's lines also stand in for its resulting contents wherever they are left out for size.
#[derive(Clone, Debug, Serialize)]
pub struct Excerpt {
    /// Inclusive and 1-based, in order, with overlapping and adjacent ranges merged.
    pub lines: Vec<[usize; 2]>,
    /// What the model receives as the file's contents: a note naming the lines, and those lines,
    /// each prefixed with its number so that citations keep the file's numbering. An error when
    /// the file's current contents do not hold the lines.
    #[serde(skip)]
    contents: Result<serde_json::Value, String>,
}

impl Excerpt {
    fn new(change: &Change, mut lines: Vec<[usize; 2]>) -> Result<Self> {
        let path = change.path.display();
        let text = change
            .after
            .as_deref()
            .with_context(|| format!("cannot supply lines of {path}: the change deletes it"))?;
        let all: Vec<&str> = text.split_inclusive('\n').collect();
        lines.sort_unstable();
        let mut merged: Vec<[usize; 2]> = Vec::new();
        for [start, end] in lines {
            ensure!(
                1 <= start && start <= end && end <= all.len(),
                "{path}:{start}-{end} is not within the file's {} lines",
                all.len()
            );
            match merged.last_mut() {
                Some(last) if start <= last[1] + 1 => last[1] = last[1].max(end),
                _ => merged.push([start, end]),
            }
        }
        let mut numbered = String::new();
        for &[start, end] in &merged {
            for (number, line) in (start..=end).zip(&all[start - 1..end]) {
                numbered.push_str(&format!("{number}\t{line}"));
            }
        }
        let contents = serde_json::json!({
            "excerpt": format!(
                "Only lines {} of {} were supplied. Each line starts with its line number and a tab.",
                line_ranges(&merged),
                all.len()
            ),
            "lines": numbered,
        });
        Ok(Self {
            lines: merged,
            contents: Ok(contents),
        })
    }

    /// What the model receives as the file's contents, or why the file cannot supply the lines.
    pub(crate) fn contents(&self) -> Result<serde_json::Value> {
        self.contents.clone().map_err(anyhow::Error::msg)
    }
}

/// Inclusive line ranges as text, such as `3, 10-12`.
pub fn line_ranges(lines: &[[usize; 2]]) -> String {
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
    /// Supply repository files as context, each once. A file named without lines is supplied
    /// whole; otherwise only every range named for it. Lines the file no longer holds make it a
    /// capture error, so recapturing an earlier check shows its input changed; a new check or
    /// export refuses them with [`Plan::admit_context`].
    pub fn with_context(mut self, requests: &[SpanRequest]) -> Result<Self> {
        let mut ranges: BTreeMap<PathBuf, Vec<[usize; 2]>> = (self.excerpts.iter())
            .map(|(path, excerpt)| (path.clone(), excerpt.lines.clone()))
            .collect();
        for request in requests {
            let path = &request.path;
            ensure!(
                is_source(&self.root, path)?,
                "context paths must be repository source files: {}",
                path.display()
            );
            let known = self.context.iter().any(|change| &change.path == path);
            match request.lines {
                None => {
                    ranges.remove(path);
                }
                // A file already supplied whole stays whole.
                Some(_) if known && !ranges.contains_key(path) => {}
                Some((start, end)) => ranges.entry(path.clone()).or_default().push([start, end]),
            }
            if known {
                continue;
            }
            self.context.push(self.read_change(path)?);
        }
        self.excerpts = (self.context.iter())
            .filter_map(|change| {
                let lines = ranges.remove(&change.path)?;
                let excerpt = Excerpt::new(change, lines.clone()).unwrap_or_else(|error| Excerpt {
                    lines,
                    contents: Err(format!("{error:#}")),
                });
                Some((change.path.clone(), excerpt))
            })
            .collect();
        Ok(self)
    }

    /// Refuse context that a new check or export cannot send: a file that does not exist, lines
    /// the file does not hold, or more than the file input limit, as the model receives it, all
    /// together.
    pub fn admit_context(&self) -> Result<()> {
        let mut inputs = Vec::new();
        for change in &self.context {
            ensure!(
                change.before.is_some() || change.after.is_some(),
                "context file does not exist: {}",
                change.path.display()
            );
            inputs.push(self.context_input(change)?);
        }
        let bytes = serde_json::to_vec(&inputs)?.len();
        ensure!(
            bytes <= ev_grep_core::MAX_FILE_BYTES,
            "supplementary context is {bytes} bytes as the model receives it; limit is {} bytes; \
             supply only the lines needed with --context PATH:START-END",
            ev_grep_core::MAX_FILE_BYTES
        );
        Ok(())
    }

    /// What the model receives for a `--context` file: its path and resulting contents, or only
    /// the lines of an excerpt.
    pub(crate) fn context_input(&self, change: &Change) -> Result<serde_json::Value> {
        Ok(match self.excerpts.get(&change.path) {
            Some(excerpt) => serde_json::json!({
                "path": change.path,
                "contents": excerpt.contents()?,
            }),
            None => change.current_input(),
        })
    }

    /// Whether `contract` governs `path` at its current location or, for a rename, its previous one.
    pub fn governs(&self, contract: &Contract, path: &Path) -> bool {
        discovery::governs(contract, &self.previous_paths, path)
    }

    /// Contract documents the diff deletes. Moving one counts too, unless discovery still finds a
    /// contract document at its new path.
    pub fn deleted_contracts(&self) -> Vec<PathBuf> {
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

    /// Whether a diff changes none of the contract's files and not the contract itself, so the
    /// contract is unaffected without a model request.
    pub fn untouched(&self, contract: &Contract) -> bool {
        self.mode == crate::Mode::Diff
            && !self.files.iter().any(|path| self.governs(contract, path))
            && !self.contract_changes.contains(&contract.path)
    }

    pub fn requested_scope(&self, contract: &Contract) -> crate::RequestedScope {
        let selected: BTreeSet<_> = self.files.iter().collect();
        let mut scoped_candidates = self
            .candidate_files
            .iter()
            .filter(|path| self.governs(contract, path))
            .peekable();
        let covers_empty_scope = self.selected_paths.is_empty()
            || self
                .selected_paths
                .iter()
                .any(|selector| contract.scope.starts_with(selector));
        let covers_scope = (scoped_candidates.peek().is_some() || covers_empty_scope)
            && scoped_candidates.all(|path| selected.contains(path));
        if covers_scope {
            crate::RequestedScope::FullContract
        } else {
            crate::RequestedScope::SelectedSubjects
        }
    }

    pub(crate) fn read_change(&self, path: &Path) -> Result<crate::Change> {
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

/// Whether a caller may supply this path as repository source, as `--context` or as a record's
/// evidence: relative, outside excluded and contract folders, and not leading out of the canonical
/// `root` through a symbolic link.
pub(crate) fn is_source(root: &Path, path: &Path) -> Result<bool> {
    let relative = path
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
        && allowed(path)
        && !is_contract(path);
    let full = root.join(path);
    Ok(relative && (!full.exists() || full.canonicalize()?.starts_with(root)))
}
