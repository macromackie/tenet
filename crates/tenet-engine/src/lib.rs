//! Scoped file selection and ordered, bounded contract evaluation.

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

pub use change::{Change, ChangeKind, Mode};
pub use check::{Options, Prediction, carry, predict, run};
pub use discovery::{Coverage, ExcludedPath, Exclusion, Plan, Selection, discover_contracts, plan};
pub use ev_grep_core::{
    Assessment, Candidate, DecisionBatch, Evaluator, MAX_FILE_BYTES, MIN_CONFIDENCE, Outcome,
    Question,
};
pub use evidence::{EvidencePacket, MAX_CAPTURE_BYTES, input_status};
pub use failure::{FailureKind, RequestFailure};
pub use plan::{Excerpt, SpanRequest, line_ranges};
pub use report::{
    CheckResult, ConclusionReason, ContextSummary, ContractResult, ContractState, ContractStatus,
    Event, Omitted, Pending, RequestedScope, RunInfo, Stage, Status, Summary, violation,
};
pub use review::Review;
pub use snapshot::PreparedSnapshot;
