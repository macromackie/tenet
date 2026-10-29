//! Why the review log cannot be read or extended, or why it refuses a record.

use std::{io, path::PathBuf, time::SystemTimeError};

use tenet_contracts::DocumentError;

use super::{LogError, StoreError, Unchecked};
use crate::{
    ContractStatus,
    DiscoveryError,
    GitError,
    Mode,
    RequestedScope,
    evidence::{ChangeError, EvidenceError},
    plan::ContextError,
    report::Causes,
};

/// Why the review log cannot be read or extended, or why it refuses a record.
#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    #[error("{name:?} stands in several checks of this review; name the paths of one: {choices:?}")]
    Ambiguous { name: String, choices: String },
    #[error(transparent)]
    Change(#[from] ChangeError),
    #[error(transparent)]
    Clock(#[from] SystemTimeError),
    #[error(transparent)]
    Context(#[from] ContextError),
    #[error("{:?} was deleted; cite it without lines", .0.display())]
    DeletedLines(PathBuf),
    #[error(transparent)]
    Discovery(#[from] DiscoveryError),
    #[error("r{0:?} appears twice")]
    DuplicateRevision(u64),
    #[error("this review keeps no state")]
    Ephemeral,
    #[error(transparent)]
    Evidence(#[from] EvidenceError),
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("its id does not match its contents")]
    IdMismatch,
    #[error(transparent)]
    Inspect(io::Error),

    #[error(
        "record {id:?} for {contract:?} is invalid: {:#}. Tenet trusts no record that \
         `tenet record` would refuse, and never edits the log. Move the review \
         state directory aside, run tenet check, and record the review again",
        Causes(&**.reason)
    )]
    InvalidRecord {
        id: String,
        contract: String,
        reason: Box<Self>,
    },
    #[error("invalid project root")]
    InvalidRoot(#[source] io::Error),
    #[error("{:?}:{number:?}", .path.display())]
    Line {
        path: PathBuf,
        number: usize,
        source: Box<Self>,
    },
    #[error("{:?}:{start:?}-{end:?}: lines are numbered from 1", .path.display())]
    LineZero {
        path: PathBuf,
        start: usize,
        end: usize,
    },
    #[error(transparent)]
    Log(#[from] LogError),
    #[error(
        "the message is {0:?} bytes; the limit is {max:?}",
        max = super::record::MAX_MESSAGE_BYTES
    )]
    LongMessage(usize),
    #[error(
        "the message contains control, bidirectional, or line separator characters; only newlines \
         and tabs are allowed"
    )]
    MessageCharacters,
    #[error("this review keeps no captured contents")]
    NoBlobs,

    #[error("no checks recorded; run tenet check first")]
    NoChecks,
    #[error("{:?} has no captured contents", .0.display())]
    NoContents(PathBuf),
    #[error("cite the code that supports {:?} with --evidence PATH:START-END", .0.as_str())]
    NoEvidence(ContractStatus),
    #[error("state what the evidence shows with --message")]
    NoMessage,
    #[error("no review state in {:?}; run tenet check first", .0.display())]
    NoState(PathBuf),
    // A record `tenet record` refuses, before it is written or when the log is read.
    #[error("{0:?} has not been checked; run tenet check first")]
    NotChecked(String),
    #[error("{0:?} is not part of the current review; see tenet status")]
    NotInReview(String),
    #[error(
        "{:?} is not a file r{rev:?} assessed for {contract:?}; cite one of: {:?}",
        .path.display(),
        .files.join(", ")
    )]
    NotScoped {
        path: PathBuf,
        rev: u64,
        contract: String,
        files: Vec<String>,
    },
    #[error("evidence paths must be repository source files: {:?}", .0.display())]
    NotSource(PathBuf),
    #[error(
        "this diff check selected no file {contract:?} governs, so nothing in it shows {:?}. \
         Check the contract in full with `tenet check --contract {contract:?}`, settle that \
         check as verified, then record this one",
        .conclusion.as_str()
    )]
    NothingSelected {
        contract: String,
        conclusion: ContractStatus,
    },
    #[error("r{rev:?} assessed {contract:?} with other input")]
    OtherInput { rev: u64, contract: String },
    #[error(
        "r{rev:?} supplied only lines {:?} of {:?}; cite lines within them",
        crate::format_line_ranges(.excerpt),
        .path.display()
    )]
    OutsideExcerpt {
        rev: u64,
        excerpt: Vec<[usize; 2]>,
        path: PathBuf,
    },
    #[error("{:?}:{start:?}-{end:?} ends after the file's {count:?} lines", .path.display())]
    PastEnd {
        path: PathBuf,
        start: usize,
        end: usize,
        count: usize,
    },
    #[error("cannot read evidence {:?}", .path.display())]
    ReadEvidence { path: PathBuf, source: io::Error },
    #[error("{:?}:{start:?}-{end:?}: the range starts after it ends", .path.display())]
    Reversed {
        path: PathBuf,
        start: usize,
        end: usize,
    },
    #[error("{contract:?}: scoped files changed after r{rev:?}; run tenet check before recording")]
    Stale { contract: String, rev: u64 },
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(
        "the model flagged {:?}; cite the lines there that show {:?}",
        .flagged.join(", "),
        .conclusion.as_str()
    )]
    Unanswered {
        flagged: Vec<String>,
        conclusion: ContractStatus,
    },
    // The review as it stands in the working tree.
    #[error(transparent)]
    Unchecked(#[from] Unchecked),
    #[error(
        "cite a line this change wrote, a line beside it, or a file it deleted or emptied, to \
         show {:?}: {citations:?}",
        .conclusion.as_str()
    )]
    UncitedChange {
        conclusion: ContractStatus,
        /// The changed ranges to cite, as many as a refusal lists.
        citations: String,
    },
    #[error(
        "{:?} does not fit a {:?} check of {:?}; record one of: {:?}",
        .conclusion.as_str(),
        .mode.as_str(),
        .scope.describe(),
        .allowed.iter().map(|choice| choice.as_str()).collect::<Vec<_>>().join(", ")
    )]
    Unfit {
        conclusion: ContractStatus,
        mode: Mode,
        scope: RequestedScope,
        allowed: &'static [ContractStatus],
    },
    #[error("r{0:?} is not in the review log")]
    UnknownRevision(u64),
}

impl ReviewError {
    /// The contract document a discovery refused, so a report can point into it.
    #[must_use]
    #[inline]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The remaining variants take no action here, and listing each would repeat the \
                  enum."
    )]
    pub const fn as_document(&self) -> Option<&DocumentError> {
        match self {
            Self::Discovery(error) => error.as_document(),
            _ => None,
        }
    }
}
