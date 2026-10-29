//! Entries in `.tenet/events.jsonl`, the review's only source of truth.

use alloc::collections::BTreeSet;
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{ConclusionReason, ContractStatus, Mode, Omitted, RequestedScope, Status, evidence};

/// The format `check` and `record` write. Each revision lists its captured files once, and its
/// assessments refer to them by position.
pub(super) const FORMAT: u64 = 2;

/// Why a line of the review log is not an entry this Tenet reads.
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("the assessment of {contract:?} names a file r{rev:?} did not capture")]
    UncapturedFile { contract: String, rev: u64 },
    #[error("unknown entry type {0:?}")]
    UnknownEntry(String),
    #[error("format {0:?} is not one this Tenet reads; upgrade Tenet")]
    UnknownFormat(u64),
    #[error("the assessment of {contract:?} does not follow r{rev:?}")]
    Unordered { contract: String, rev: u64 },
}

/// One line of the review log.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Entry {
    Assessment(Assessment),
    Record(Record),
    Revision(Revision),
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
/// One `tenet check`. Its selection lets later commands recapture exactly what it assessed.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Revision {
    pub rev: u64,
    /// Unix seconds.
    pub at: u64,
    /// The branch and base this revision belongs to, for display.
    pub review: String,
    pub base: Option<String>,
    pub paths: Vec<PathBuf>,
    pub contract: Option<String>,
    pub context: Vec<crate::SpanRequest>,
    pub mode: Mode,
    pub base_oid: Option<String>,
    pub head_oid: Option<String>,
    /// The provider and model that answered; absent for `check --no-model`.
    pub model: Option<String>,
    /// The confidence below which the model's answers were routed to unresolved; absent for
    /// `check --no-model` and for revisions written before Tenet recorded it.
    pub min_confidence: Option<f64>,
    pub tenet: String,
    /// Every file this revision's assessments cover, each once. Format 1 listed them in each
    /// assessment instead.
    #[serde(default)]
    pub files: Vec<CapturedFile>,
}

impl Revision {
    /// Whether this revision's path selection includes everything `earlier` selected, so its
    /// assessment of a contract replaces `earlier`'s. No paths select everything.
    #[must_use]
    #[inline]
    pub fn is_covering(&self, earlier: &Self) -> bool {
        let selected = build_selectors(&self.paths);
        let earlier_selectors = build_selectors(&earlier.paths);
        selected.is_empty()
            || (!earlier_selectors.is_empty()
                && earlier_selectors
                    .iter()
                    .all(|path| selected.iter().any(|selector| path.starts_with(selector))))
    }

    /// Revisions against the same base commit review the same change, whichever paths and
    /// contracts they selected. Full checks have no base and form one review.
    #[must_use]
    #[inline]
    pub fn is_same_review(&self, other: &Self) -> bool {
        self.base_oid == other.base_oid
    }

    /// Whether these are the revision's selected paths, however they were spelled.
    #[must_use]
    #[inline]
    pub fn is_selecting(&self, paths: &[PathBuf]) -> bool {
        build_selectors(&self.paths) == build_selectors(paths)
    }

    /// The files one of this revision's assessments covers: its selected files, then its
    /// supporting files.
    #[expect(
        clippy::indexing_slicing,
        reason = "Assessment file indexes point into this log's `files`, which the store \
                  validates on read."
    )]
    #[inline]
    pub fn list_files<'log>(
        &'log self,
        assessment: &'log Assessment,
    ) -> impl Iterator<Item = (Role, &'log CapturedFile)> {
        let selected = assessment
            .selected
            .iter()
            .map(|&index| (Role::Selected, index));
        let support = assessment
            .support
            .iter()
            .map(|&index| (Role::Support, index));
        selected
            .chain(support)
            .map(|(role, index)| (role, &self.files[index]))
    }
}

fn build_selectors(paths: &[PathBuf]) -> BTreeSet<PathBuf> {
    paths
        .iter()
        .map(|path| {
            path.components()
                .filter(|part| *part != Component::CurDir)
                .collect::<PathBuf>()
        })
        .filter(|path| path != Path::new(""))
        .collect()
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
/// One contract's assessment in one revision, bound to its exact input.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Assessment {
    pub rev: u64,
    pub contract: String,
    /// The contract document's hash; its text is kept in `.tenet/blobs/`.
    pub contract_hash: String,
    pub input: String,
    pub scope: RequestedScope,
    /// The model's conclusion; absent when the contract was captured without one.
    pub conclusion: Option<Conclusion>,
    /// The revision whose model conclusion for this same input `check --no-model` reused.
    pub carried_from: Option<u64>,
    /// Positions in the revision's `files` of the files this contract selected.
    pub selected: Vec<usize>,
    /// Positions of the `--context` files that supported them.
    pub support: Vec<usize>,
    pub results: Vec<FileResult>,
}

/// A conclusion that a record states for one contract.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Conclusion {
    pub status: ContractStatus,
    pub reason_code: ConclusionReason,
    pub reason: String,
    /// The model's probability of a violation in the requested scope.
    pub violation: Option<f64>,
    /// The combined assessment's raw answer before confidence routing; absent when it did not run
    /// and in revisions written before Tenet recorded it.
    pub answer: Option<Answer>,
    pub error: Option<String>,
    pub conflicting_files: Vec<String>,
}

/// A model's raw choice and its confidence in it.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct Answer {
    pub choice: ev_grep_core::Outcome,
    pub confidence: f64,
}

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
            && (self.after.as_deref()).is_none_or(|after| after == evidence::hash_content(""))
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

/// One file's assessment in a recorded revision.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileResult {
    pub path: String,
    pub status: Status,
    /// The model's probability that this file violates the contract.
    pub violation: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omitted: Vec<Omitted>,
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
/// An agent's or person's conclusion for one contract input. Never edited; a later record for
/// the same input supersedes it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Record {
    pub id: String,
    pub at: u64,
    pub rev: u64,
    pub contract: String,
    pub input: String,
    pub conclusion: ContractStatus,
    pub evidence: Vec<Span>,
    pub message: String,
    /// The model's conclusion for this input when the record was made.
    pub model: Option<ContractStatus>,
}

/// A cited span of lines in one file.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Span {
    pub path: PathBuf,
    /// Inclusive, 1-based; absent when the evidence is the whole file.
    pub lines: Option<[usize; 2]>,
    pub blob: String,
}

/// Parse one line of the log.
///
/// An assessment must follow `revision`, its revision, and refer only to files it captured. Format
/// 1, which Tenet 0.8 wrote, listed every assessment's files in full; those move into `revision`,
/// so records made with Tenet 0.8 keep settling their inputs, and `moved_files` remembers where
/// each went.
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
pub(super) fn parse(
    format: u64,
    line: &str,
    revision: Option<&mut Revision>,
    moved_files: &mut HashMap<CapturedFile, usize>,
) -> Result<Entry, LogError> {
    // The type selects the struct to parse. An internally tagged enum would buffer the line first,
    // which loses the arbitrary-precision numbers serde_json is built with here.
    #[derive(Deserialize)]
    struct Kind {
        #[serde(rename = "type")]
        kind: String,
    }

    #[derive(Deserialize)]
    struct Assessment1 {
        carried_from: Option<u64>,
        conclusion: Option<Conclusion>,
        contract: String,
        contract_hash: String,
        files: Vec<ScopedFile>,
        input: String,
        results: Vec<FileResult>,
        rev: u64,
        scope: RequestedScope,
    }

    let Kind { kind } = serde_json::from_str(line)?;
    Ok(match (format, kind.as_str()) {
        (1 | FORMAT, "revision") => {
            moved_files.clear();
            Entry::Revision(serde_json::from_str(line)?)
        },
        (1 | FORMAT, "record") => Entry::Record(serde_json::from_str(line)?),
        (1 | FORMAT, "assessment") => {
            let (mut assessment, files) = if format == 1 {
                let old: Assessment1 = serde_json::from_str(line)?;
                let assessment = Assessment {
                    rev: old.rev,
                    contract: old.contract,
                    contract_hash: old.contract_hash,
                    input: old.input,
                    scope: old.scope,
                    conclusion: old.conclusion,
                    carried_from: old.carried_from,
                    selected: Vec::new(),
                    support: Vec::new(),
                    results: old.results,
                };
                (assessment, old.files)
            } else {
                (serde_json::from_str(line)?, Vec::new())
            };
            let (rev, contract) = (assessment.rev, &assessment.contract);
            let Some(found) = revision.filter(|candidate| candidate.rev == rev) else {
                return Err(LogError::Unordered {
                    contract: contract.clone(),
                    rev,
                });
            };
            if format == 1 {
                (assessment.selected, assessment.support) =
                    add_files(&mut found.files, moved_files, files);
            }
            if !(assessment.selected.iter())
                .chain(&assessment.support)
                .all(|&position| position < found.files.len())
            {
                return Err(LogError::UncapturedFile {
                    contract: contract.clone(),
                    rev,
                });
            }
            Entry::Assessment(assessment)
        },
        (1 | FORMAT, name) => return Err(LogError::UnknownEntry(name.to_owned())),
        _ => return Err(LogError::UnknownFormat(format)),
    })
}

/// Add one assessment's scoped files to a revision's `files`, keeping one copy of each, and return
/// the positions of its selected files and of its supporting files.
///
/// `positions` remembers where each file already is.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "The files vector is nonempty after the push before it."
)]
pub(super) fn add_files(
    files: &mut Vec<CapturedFile>,
    positions: &mut HashMap<CapturedFile, usize>,
    scoped: Vec<ScopedFile>,
) -> (Vec<usize>, Vec<usize>) {
    let (mut selected, mut support) = (Vec::new(), Vec::new());
    for ScopedFile { role, file } in scoped {
        let position = *positions.entry(file.clone()).or_insert_with(|| {
            files.push(file);
            files.len() - 1
        });
        match role {
            Role::Selected => selected.push(position),
            Role::Support => support.push(position),
        }
    }
    (selected, support)
}
