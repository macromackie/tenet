use serde::{Deserialize, Serialize};

use crate::HunkRange;

/// A model's raw choice and confidence before routing.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct Answer {
    pub choice: ev_grep_core::Outcome,
    pub confidence: f64,
}

/// Advisory evidence from one group of complete hunks of an oversized diff.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HunkResult {
    pub applicability: Option<Answer>,
    pub evidence_hash: Option<String>,
    pub failure: Option<crate::RequestFailure>,
    pub input_bytes: usize,
    /// Encoded input bound used when preparing this group; older records used the standard bound.
    #[serde(default = "read_standard_input_limit")]
    pub input_limit_bytes: usize,
    pub ranges: Vec<HunkRange>,
    pub reason: Option<String>,
    pub status: Status,
    pub verification: Option<Answer>,
    pub violation: Option<f64>,
}

/// A stage of a check run, as progress events name it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Applicability,
    Completeness,
    Verification,
}

/// The outcome of checking one file against one contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Error,
    Fail,
    Incomplete,
    NotApplicable,
    Pass,
    /// No text to judge: an empty, binary, non-UTF-8, or symbolic-link file.
    Skipped,
    Uncertain,
}

/// Part of a file's change left out of its model input to fit the input limit.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Omitted {
    /// The prior contents; the patch and resulting contents remain.
    Before,
    /// The resulting contents too; only the patch remains, with any lines of the file named with
    /// `--context`.
    Contents,
}

/// One file's result for one contract, as a report lists it.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Clone, Debug, Serialize)]
pub struct CheckResult {
    pub mode: crate::Mode,
    pub change_kind: Option<crate::ChangeKind>,
    pub previous_path: Option<String>,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    pub contract: String,
    pub contract_path: String,
    pub contract_hash: String,
    pub path: String,
    pub input_bytes: Option<usize>,
    pub input_limit_bytes: usize,
    pub evidence_hash: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub omitted: Vec<Omitted>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hunks: Vec<HunkResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hunk_summary: Option<super::Summary>,
    pub status: Status,
    pub message: String,
    pub reason: Option<String>,
    /// Why the file's request returned no answer, when it failed.
    pub failure: Option<crate::RequestFailure>,
    pub applicability: Option<ev_grep_core::Assessment>,
    pub applicability_routing: Option<ev_grep_core::Routing>,
    pub verification: Option<ev_grep_core::Assessment>,
    pub verification_routing: Option<ev_grep_core::Routing>,
    pub request: Option<ev_grep_core::RequestInfo>,
    /// Original shared request identity; its usage is counted once by the batch event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_request: Option<String>,
    /// Requests charged exclusively to this file, including failed hunk requests; shared usage
    /// belongs to its batch event and the run summary.
    pub requests: usize,
    /// The answers came from the review's answer cache; no request was sent.
    pub cached: bool,
    pub elapsed_ms: u128,
}

const fn read_standard_input_limit() -> usize {
    crate::MAX_FILE_BYTES
}

/// The probability an answer assigns to a violation, whatever its routed outcome.
#[must_use]
#[inline]
pub fn estimate_violation(assessment: &ev_grep_core::Assessment) -> f64 {
    assessment
        .probabilities
        .get(&ev_grep_core::Outcome::Match)
        .copied()
        .unwrap_or(0.0)
}
