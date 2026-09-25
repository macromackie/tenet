//! Scoped file selection and ordered, bounded contract evaluation.

mod discovery;
mod git;
mod report;
mod runner;

pub use discovery::{Plan, Selection, discover_contracts, plan};
pub use report::{CheckResult, Event, RunInfo, Stage, Status, Summary};
pub use runner::{Options, run, run_examples};
