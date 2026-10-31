use serde::{Deserialize, Serialize};

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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub omitted: Vec<Omitted>,
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
    /// The answers came from the review's answer cache; no request was sent.
    pub cached: bool,
    pub elapsed_ms: u128,
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
