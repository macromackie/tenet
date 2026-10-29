//! Scoped file selection and ordered, bounded contract evaluation.

extern crate alloc;

pub mod adoption;
mod change;
mod check;
mod discovery;
mod evidence;
mod failure;
mod git;
mod plan;
mod report;
pub mod review;
mod snapshot;
mod telemetry;

pub use change::{Change, ChangeKind, InputLimit, Mode, SourceError, Unreadable};
pub use check::{CheckError, CombinedError, Options, Prediction, apply_review, estimate, run};
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
    InputError,
    MAX_CAPTURE_BYTES,
    classify_input_status,
};
pub use failure::{ClassifyRequest, FailureKind, RequestFailure};
pub use git::GitError;
pub use plan::{ContextError, Excerpt, ExcerptError, SpanError, SpanRequest, format_line_ranges};
pub use report::{
    Causes,
    CheckResult,
    ConclusionReason,
    ContextSummary,
    ContractResult,
    ContractState,
    ContractStatus,
    EVENTS_VERSION,
    Event,
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
