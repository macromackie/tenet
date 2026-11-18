//! Scoped file selection and ordered, bounded contract evaluation.

#![expect(
    clippy::multiple_crate_versions,
    reason = "SHA-256 and BLAKE3 pin different cpufeatures versions listed in deny.toml."
)]

extern crate alloc;

pub mod adoption;
mod change;
mod check;
mod discovery;
mod evidence;
mod failure;
mod git;
mod lock;
pub mod mechanical;
mod plan;
mod report;
pub mod review;
mod snapshot;
mod telemetry;

pub use change::{
    Change,
    ChangeKind,
    Excerpt,
    ExcerptError,
    HunkRange,
    InputLimit,
    MAX_CAPTURE_BYTES,
    Mode,
    SourceError,
    Unreadable,
    format_line_ranges,
};
pub use check::{
    CheckError,
    Options,
    Prediction,
    PredictionError,
    apply_review,
    estimate,
    estimate_with_measure,
    run,
};
pub use discovery::{
    Coverage,
    DiscoveryError,
    ExcludedPath,
    Exclusion,
    Plan,
    Selection,
    discover_contracts,
    plan,
};
pub use ev_grep_core::{
    Assessment,
    Candidate,
    ContextProfile,
    DecisionBatch,
    Evaluator,
    MAX_FILE_BYTES,
    MIN_CONFIDENCE,
    Outcome,
    Question,
};
pub use evidence::{
    CaptureError,
    ChangeError,
    EvidenceError,
    EvidencePacket,
    HunkInput,
    InputError,
    classify_input_status,
};
pub use failure::{ClassifyRequest, FailureKind, RequestFailure};
pub use git::GitError;
pub use plan::{ContextError, SpanError, SpanRequest};
pub use report::{
    Answer,
    Causes,
    CheckResult,
    CombinedStatus,
    ConclusionReason,
    ContextSummary,
    ContractResult,
    ContractState,
    ContractStatus,
    EVENTS_VERSION,
    Event,
    HunkResult,
    Omitted,
    Pending,
    RequestedScope,
    RunInfo,
    Stage,
    Status,
    Summary,
    estimate_violation,
};
pub use review::Review;
pub use snapshot::{PreparedSnapshot, SnapshotError};
