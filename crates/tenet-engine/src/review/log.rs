//! Entries in `.tenet/events.jsonl`, the review's only source of truth.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{ConclusionReason, ContractStatus, Mode, RequestedScope, Status};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Entry {
    Revision(Revision),
    Assessment(Assessment),
    Record(Record),
}

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
    pub context: Vec<PathBuf>,
    pub mode: Mode,
    pub base_oid: Option<String>,
    pub head_oid: Option<String>,
    /// The provider and model that answered; absent for `check --no-model`.
    pub model: Option<String>,
    pub tenet: String,
}

impl Revision {
    /// Revisions with the same base and paths review the same change, whichever contracts they ran.
    pub fn same_review(&self, other: &Self) -> bool {
        self.base == other.base && self.paths == other.paths
    }
}

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
    /// An earlier revision whose conclusion for this same input `check --no-model` reused.
    pub carried_from: Option<u64>,
    pub files: Vec<ScopedFile>,
    pub results: Vec<FileResult>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Conclusion {
    pub status: ContractStatus,
    pub reason_code: ConclusionReason,
    pub reason: String,
    pub confidence: Option<f64>,
    pub error: Option<String>,
    pub conflicting_files: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Selected,
    Support,
}

/// A captured file's identity. Contents live in `.tenet/blobs/` under these hashes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopedFile {
    pub path: PathBuf,
    pub role: Role,
    pub previous_path: Option<PathBuf>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileResult {
    pub path: String,
    pub status: Status,
    pub confidence: Option<f64>,
}

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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Span {
    pub path: PathBuf,
    /// Inclusive, 1-based; absent when the evidence is the whole file.
    pub lines: Option<[usize; 2]>,
    pub blob: String,
}
