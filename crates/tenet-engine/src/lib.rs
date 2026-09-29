//! Scoped file selection and ordered, bounded contract evaluation.

mod change;
mod check;
mod discovery;
mod git;
mod plan;
mod report;
mod snapshot;

pub use change::{Change, ChangeKind, Mode};
pub use check::{Options, run};
pub use discovery::{Plan, Selection, discover_contracts, plan};
pub use report::{
    CheckResult, ContractResult, ContractStatus, Event, RunInfo, Stage, Status, Summary,
};
pub use snapshot::PreparedSnapshot;
