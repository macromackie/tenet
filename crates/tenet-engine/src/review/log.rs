//! Entries in `.tenet/events.jsonl`, the review's only source of truth.

mod parsing;
mod revision;

use std::path::PathBuf;

pub(super) use parsing::{add_files, parse};
pub use revision::Revision;
use serde::{Deserialize, Serialize};

use crate::{
    ConclusionReason,
    ContractStatus,
    Omitted,
    RequestedScope,
    Status,
    evidence::{CapturedFile, Role, ScopedFile},
};

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
