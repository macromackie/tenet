//! An agent's conclusion for exact captured input, admitted before append and on every replay.

mod submission;
mod validation;

use std::path::PathBuf;

pub use submission::record;
pub(super) use validation::validate;
use validation::{compute_id, select_scoped};

use super::Record;
use crate::{ContractStatus, Mode, RequestedScope, SpanRequest};

/// The longest message a record keeps, in bytes.
pub(super) const MAX_MESSAGE_BYTES: usize = 4096;

/// What `tenet record` asks to record for one contract.
#[derive(Debug)]
pub struct RecordRequest {
    pub conclusion: ContractStatus,
    pub contract: String,
    pub evidence: Vec<SpanRequest>,
    pub message: String,
    /// The selected paths of the check to settle; empty for the contract's only selection.
    pub paths: Vec<PathBuf>,
}

/// A record that now settles its input, and the earlier record for that input it replaced.
#[derive(Debug)]
pub struct Recorded {
    pub record: Record,
    pub replaces: Option<Record>,
}

/// Conclusions a record may state, which are those a check of the same mode and scope can reach.
#[must_use]
#[inline]
pub const fn list_conclusions(mode: Mode, scope: RequestedScope) -> &'static [ContractStatus] {
    use ContractStatus::{Clear, Failed, Preserved, Unaffected, Unresolved, Verified};
    match (mode, scope) {
        (_, RequestedScope::SelectedSubjects) => &[Clear, Failed, Unresolved],
        (Mode::Full, RequestedScope::FullContract) => &[Verified, Failed, Unresolved],
        (Mode::Diff, RequestedScope::FullContract) => &[Preserved, Unaffected, Failed, Unresolved],
    }
}
