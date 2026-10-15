//! Scoped file selection and ordered, bounded contract evaluation.

pub mod adoption;
mod change;
mod check;
mod discovery;
mod evidence;
mod git;
mod plan;
mod report;
pub mod review;
mod snapshot;

pub use change::{Change, ChangeKind, Mode};
pub use check::{Options, carry, run};
pub use discovery::{ExcludedPath, Exclusion, Plan, Selection, discover_contracts, plan};
pub use ev_grep_core::{MAX_FILE_BYTES, MIN_CONFIDENCE, Outcome};
pub use evidence::{EvidencePacket, MAX_CAPTURE_BYTES, input_status};
pub use report::{
    CheckResult, ConclusionReason, ContextSummary, ContractResult, ContractState, ContractStatus,
    Event, Omitted, Pending, RequestedScope, RunInfo, Stage, Status, Summary, violation,
};
pub use review::Review;
pub use snapshot::PreparedSnapshot;
