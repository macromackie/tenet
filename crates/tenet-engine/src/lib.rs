//! Scoped file selection and ordered, bounded contract evaluation.

mod change;
mod conclusion;
mod discovery;
mod git;
mod plan;
mod report;
mod runner;
mod snapshot;

pub use change::{Change, ChangeKind, Mode};
pub use discovery::{Plan, Selection, discover_contracts, plan};
pub use report::{
    CheckResult, ContractResult, ContractStatus, Event, RunInfo, Stage, Status, Summary,
};
pub use runner::{Options, run, run_examples};
pub use snapshot::PreparedSnapshot;
