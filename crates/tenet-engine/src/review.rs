//! Local review state: what each check assessed, keyed by exact input, and what agents recorded.
//!
//! `.tenet/events.jsonl` is the only source of truth. `answers.jsonl` caches raw model answers and
//! `blobs/` holds captured file contents; deleting either loses no conclusion.

mod answers;
mod commit;
mod current;
mod error;
mod history;
mod log;
mod open;
mod record;
mod replay;
mod state;
mod store;

use std::{
    collections::HashMap,
    time::{SystemTime, SystemTimeError, UNIX_EPOCH},
};

pub(crate) use answers::Answers;
pub use answers::Pruned;
pub use current::{Current, CurrentContract, Unchecked, resolve_current};
#[expect(
    clippy::module_name_repetitions,
    reason = "Callers name it from the public `review` module, beside the other crates' error \
              types."
)]
pub use error::ReviewError;
pub use log::{
    Answer,
    Assessment,
    Conclusion,
    Entry,
    FileResult,
    LogError,
    Record,
    Revision,
    Span,
};
pub use record::{RecordRequest, Recorded, list_conclusions, record};
use store::Store;
pub use store::{DIRECTORY, MAX_BLOB_BYTES, StoreError};

pub use crate::evidence::{CapturedFile, Role, ScopedFile};
use crate::{ContractState, ContractStatus, Selection};

/// The review state in `.tenet`: its log, records, and cached answers.
#[expect(
    clippy::module_name_repetitions,
    reason = "The crate root re-exports it as `tenet_engine::Review`, where the name repeats \
              nothing."
)]
#[derive(Debug)]
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "The sibling modules fill these fields, and the type stays private to its parent \
              module."
)]
pub struct Review {
    pub(crate) answers: Answers,
    end: u64,
    /// The log in the order it was written.
    entries: Vec<Entry>,
    /// The number of log lines read, and the byte offset after them.
    lines: usize,
    /// Each revision's position in `entries`; its assessments follow it.
    revisions: HashMap<u64, usize>,
    store: Option<Store>,
}

/// A contract's outcome in one check, before it is written to the log.
#[derive(Debug)]
pub struct Concluded {
    pub carried_from: Option<u64>,
    pub conclusion: Option<Conclusion>,
    pub contract: String,
    pub input: String,
    pub results: Vec<FileResult>,
    pub scope: crate::RequestedScope,
    pub state: ContractState,
}

/// The results and contract states a check produced.
#[derive(Debug)]
pub struct Checked {
    /// One per planned contract, in plan order.
    pub contracts: Vec<Concluded>,
    pub summary: crate::Summary,
}

impl Checked {
    /// Pending contracts, the one the model leans hardest toward violating first.
    #[must_use]
    #[inline]
    pub fn list_pending(&self) -> Vec<String> {
        let violation = |concluded: &Concluded| {
            let conclusion = concluded.conclusion.as_ref()?;
            conclusion.violation
        };
        let mut pending: Vec<_> = self
            .contracts
            .iter()
            .filter(|contract| contract.state.is_pending())
            .collect();
        pending.sort_by(|left, right| {
            let (left_violation, right_violation) = (
                violation(left).unwrap_or(-1.0_f64),
                violation(right).unwrap_or(-1.0_f64),
            );
            right_violation.total_cmp(&left_violation)
        });
        pending
            .into_iter()
            .map(|contract| contract.contract.clone())
            .collect()
    }

    /// Cancellation and execution errors take precedence over the review's states. A failed model
    /// request leaves its contract pending, so the states decide; a rerun sends it again.
    #[must_use]
    #[inline]
    pub fn summarize_overall(&self) -> OverallStatus {
        if self.summary.cancelled {
            OverallStatus::Cancelled
        } else if self.summary.errors > self.summary.request_failures {
            OverallStatus::Error
        } else {
            summarize_overall(self.contracts.iter().map(|contract| &contract.state))
        }
    }
}

/// What a check was asked to assess, kept so later commands can capture the same inputs again.
#[derive(Debug)]
pub struct CheckRequest<'plan> {
    pub context: &'plan [crate::SpanRequest],
    /// The confidence below which answers were routed to unresolved; absent without a model.
    pub min_confidence: Option<f64>,
    /// The provider and model that answered; absent for `check --no-model`.
    pub model: Option<&'plan str>,
    pub selection: &'plan Selection,
}

/// A check's or a review's overall status. The variants are declared from the lowest precedence to
/// the highest, so the greatest status of a review's contracts decides.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Variant order is the derived precedence order."
)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum OverallStatus {
    /// Every contract settled favorably.
    Passed,
    /// A contract settled unresolved.
    Unresolved,
    /// A contract settled failed.
    Failed,
    /// A contract needs a check or a record before the review is final.
    Pending,
    /// An execution error stopped part of the check.
    Error,
    /// The check was cancelled.
    Cancelled,
}

/// The overall status of a review's states: pending while anything is pending, then a settled
/// failure, then a settled unresolved conclusion, and passed when every contract settled
/// favorably.
///
/// Callers give cancellation and execution errors precedence.
#[inline]
#[expect(
    clippy::impl_trait_in_params,
    reason = "Callers pass closures and evaluators whose types they cannot name."
)]
pub fn summarize_overall<'plan>(
    states: impl IntoIterator<Item = &'plan ContractState>,
) -> OverallStatus {
    (states.into_iter())
        .map(|state| {
            match state {
                ContractState::Pending { .. } => OverallStatus::Pending,
                ContractState::Settled {
                    conclusion: ContractStatus::Failed,
                    ..
                } => OverallStatus::Failed,
                ContractState::Settled {
                    conclusion: ContractStatus::Unresolved,
                    ..
                } => OverallStatus::Unresolved,
                ContractState::Settled { .. } => OverallStatus::Passed,
            }
        })
        .max()
        .unwrap_or(OverallStatus::Passed)
}

pub(crate) fn read_clock() -> Result<u64, SystemTimeError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
