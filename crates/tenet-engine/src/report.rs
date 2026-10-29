use alloc::collections::BTreeMap;
use core::{error, fmt};
use std::path::PathBuf;

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

/// Counts of a run's results by status, and its elapsed time.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Summary {
    pub passed: usize,
    pub failed: usize,
    pub uncertain: usize,
    pub not_applicable: usize,
    /// Execution errors, failed requests included.
    pub errors: usize,
    pub incomplete: usize,
    pub skipped: usize,
    pub requests: usize,
    /// Requests that returned no answer; their contracts stay pending.
    pub request_failures: usize,
    /// Failed requests that a rerun of the check may answer.
    pub retryable_failures: usize,
    /// Requests not sent because `--max-requests` was spent; their checks are incomplete.
    pub over_budget: usize,
    /// Request batches answered from the answer cache, and conclusions `check --no-model` reused,
    /// instead of the provider.
    pub cached: usize,
    pub attempts: usize,
    pub hedges: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// A contract's assessment time; a run's whole wall-clock time, which its caller measures.
    pub elapsed_ms: u128,
    pub cancelled: bool,
    pub contracts_verified: usize,
    pub contracts_clear: usize,
    pub contracts_preserved: usize,
    pub contracts_unaffected: usize,
    pub contracts_failed: usize,
    pub contracts_unresolved: usize,
}

impl Summary {
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Counts of results, contracts and requests are bounded by the repository's files \
                  and the request budget, far below the integer's limit."
    )]
    pub(crate) fn add(&mut self, result: &CheckResult) {
        self.record_request(result.request.as_ref());
        self.record_failure(result.failure);
        self.cached += usize::from(result.cached);
        match result.status {
            Status::Pass => self.passed += 1,
            Status::Fail => self.failed += 1,
            Status::Uncertain => self.uncertain += 1,
            Status::NotApplicable => self.not_applicable += 1,
            Status::Error => self.errors += 1,
            Status::Incomplete => self.incomplete += 1,
            Status::Skipped => self.skipped += 1,
        }
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Counts of results, contracts and requests are bounded by the repository's files \
                  and the request budget, far below the integer's limit."
    )]
    pub(crate) const fn merge(&mut self, other: &Self) {
        self.passed += other.passed;
        self.failed += other.failed;
        self.uncertain += other.uncertain;
        self.not_applicable += other.not_applicable;
        self.errors += other.errors;
        self.incomplete += other.incomplete;
        self.skipped += other.skipped;
        self.requests += other.requests;
        self.request_failures += other.request_failures;
        self.retryable_failures += other.retryable_failures;
        self.over_budget += other.over_budget;
        self.cached += other.cached;
        self.attempts += other.attempts;
        self.hedges += other.hedges;
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.contracts_verified += other.contracts_verified;
        self.contracts_clear += other.contracts_clear;
        self.contracts_preserved += other.contracts_preserved;
        self.contracts_unaffected += other.contracts_unaffected;
        self.contracts_failed += other.contracts_failed;
        self.contracts_unresolved += other.contracts_unresolved;
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Counts of results, contracts and requests are bounded by the repository's files \
                  and the request budget, far below the integer's limit."
    )]
    fn record_failure(&mut self, failure: Option<crate::RequestFailure>) {
        if let Some(failed) = failure {
            self.request_failures += 1;
            self.retryable_failures += usize::from(failed.retryable());
        }
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The request, attempt and token counts are bounded by the run's request and \
                  token budgets."
    )]
    const fn record_request(&mut self, request: Option<&ev_grep_core::RequestInfo>) {
        if let Some(info) = request {
            self.attempts += info.attempts;
            self.hedges += info.hedges;
            self.input_tokens += info.input_tokens;
            self.output_tokens += info.output_tokens;
        }
    }
}

/// The settings and inputs of a check run, which its first event reports.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Debug, Serialize)]
pub struct RunInfo {
    pub version: String,
    pub root: String,
    /// Absent for `check --no-model`.
    pub provider: Option<String>,
    pub model: Option<String>,
    pub mode: String,
    pub base: Option<String>,
    pub head: Option<String>,
    pub assumption: Option<String>,
    pub snapshot: Option<String>,
    pub patch_hash: Option<String>,
    pub jobs: usize,
    pub max_requests: usize,
    pub min_confidence: f64,
    /// Contract documents the diff deletes; their requirements no longer apply.
    pub deleted_contracts: Vec<PathBuf>,
}

/// The schema version of `check`'s JSON Lines events, which each event line carries as `version`.
pub const EVENTS_VERSION: u64 = 4;

/// A progress event that a check emits as it runs.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Begin {
        run: RunInfo,
    },
    CheckStarted {
        contract: String,
        path: String,
    },
    ContractFinished {
        contract: String,
        summary: Summary,
        conclusion: Option<Box<ContractResult>>,
        state: ContractState,
    },
    ContractStarted {
        contract: String,
        path: String,
        total: usize,
    },
    Error {
        message: String,
    },
    Result {
        result: Box<CheckResult>,
    },
    Skipped {
        path: String,
        reason: String,
    },
    StageCompleted {
        contract: String,
        path: String,
        stage: Stage,
        outcome: ev_grep_core::Outcome,
    },
    StageStarted {
        contract: String,
        path: String,
        stage: Stage,
    },
    Summary {
        summary: Summary,
        /// The revision this check appended to `.tenet/events.jsonl`; absent when nothing is kept.
        revision: Option<u64>,
        /// Contracts that need a check or a record before the review is final.
        pending: Vec<String>,
        /// How much of a diff's change the contracts govern; absent for a full check.
        coverage: Option<crate::Coverage>,
        /// The process exit status the program maps the check's overall status to.
        exit_code: u8,
    },
}

/// The conclusion a contract reaches for one review.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractStatus {
    Clear,
    Failed,
    Preserved,
    Unaffected,
    Unresolved,
    Verified,
}

impl ContractStatus {
    /// Return the conclusion's name as reports and records spell it.
    #[must_use]
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Preserved => "preserved",
            Self::Clear => "clear",
            Self::Unaffected => "unaffected",
            Self::Failed => "failed",
            Self::Unresolved => "unresolved",
        }
    }

    /// Read a conclusion from its name, or return `None` for an unknown name.
    #[must_use]
    #[inline]
    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Verified,
            Self::Preserved,
            Self::Clear,
            Self::Unaffected,
            Self::Failed,
            Self::Unresolved,
        ]
        .into_iter()
        .find(|status| status.as_str() == value)
    }
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

/// Whether a check read a contract's whole scope or only a diff of it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestedScope {
    FullContract,
    SelectedSubjects,
}

impl RequestedScope {
    /// What a conclusion about this scope covers, in prose.
    #[must_use]
    #[inline]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::FullContract => "the full contract",
            Self::SelectedSubjects => "selected subjects",
        }
    }
}

/// A contract's conclusion and the results that support it.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Clone, Debug, Serialize)]
pub struct ContractResult {
    #[serde(rename = "assessment")]
    pub status: ContractStatus,
    pub scope: RequestedScope,
    /// Identifies the contract document and every scoped fact the assessment covered.
    pub input: String,
    /// The model's probability that the requested scope violates the contract.
    pub violation: Option<f64>,
    pub reason: String,
    pub reason_code: ConclusionReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextSummary>,
    #[serde(rename = "model_assessment", skip_serializing_if = "Option::is_none")]
    pub assessment: Option<ev_grep_core::Assessment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing: Option<ev_grep_core::Routing>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Why the combined assessment's request returned no answer, when it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<crate::RequestFailure>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub conflicting_files: Vec<String>,
    pub request: Option<ev_grep_core::RequestInfo>,
    /// The combined answer came from the answer cache, or `check --no-model` reused an earlier
    /// check's conclusion for the same input; no request was sent.
    pub cached: bool,
}

/// Why a contract reached its conclusion, as a stable code.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConclusionReason {
    AssessmentFailed,
    Compliance,
    ConflictingAssessments,
    ContextTooLarge,
    ContextUnavailable,
    FileChecksIncomplete,
    /// File checks found a violation the combined assessment did not confirm or could not run.
    FileViolation,
    ModelUncertain,
    NoRelevantChanges,
    RequestBudget,
    SelectedSubjectsClear,
    Violation,
}

/// The combined context a contract's check read, by size and paths.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Clone, Debug, Serialize)]
pub struct ContextSummary {
    pub input_bytes: usize,
    pub limit_bytes: usize,
    pub included_files: usize,
    pub selected_files: Vec<PathBuf>,
    pub support_files: Vec<PathBuf>,
    /// Support files supplied as excerpts, and selected files whose excerpt stood in for contents
    /// left out to fit the limit, with the inclusive line ranges supplied.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub excerpts: BTreeMap<PathBuf, Vec<[usize; 2]>>,
    /// Scoped inventory files whose contents were not supplied.
    pub omitted_files: usize,
    /// Prior contents left out of every change to fit the limit; patches remain.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub before_omitted: bool,
    /// Inventory paths summarized as per-directory file counts to fit the limit.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub inventory_counted: bool,
    /// Changed files whose resulting contents were left out to fit the limit; patches and any
    /// excerpts remain.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub contents_omitted: Vec<PathBuf>,
}

impl ContextSummary {
    /// Whether the combined input fits its limit, so the combined assessment can run.
    #[must_use]
    #[inline]
    pub const fn is_fitting(&self) -> bool {
        self.input_bytes <= self.limit_bytes
    }

    /// Whether anything was left out of a combined input that fit the limit.
    #[must_use]
    #[inline]
    pub const fn is_reduced(&self) -> bool {
        self.is_fitting()
            && (self.before_omitted || self.inventory_counted || !self.contents_omitted.is_empty())
    }
}

/// Where a contract stands in its review once records and model conclusions are combined.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ContractState {
    /// The result is not final until a check or a record settles it.
    Pending { reason: Pending },
    /// A favorable model conclusion, or a record bound to the current input.
    Settled {
        conclusion: ContractStatus,
        /// The record that settled the contract; absent when the model's conclusion did.
        record: Option<String>,
    },
}

impl ContractState {
    /// Return whether the contract still waits for a decision.
    #[must_use]
    #[inline]
    pub const fn is_pending(&self) -> bool {
        matches!(self, Self::Pending { .. })
    }
}

/// Why a contract is still pending.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Pending {
    /// The diff adds or edits the contract itself, so a diff check's favorable conclusion, which
    /// assumes the base complied, does not settle it; a full check informs the record that does.
    ContractChanged,
    /// Scoped files or the contract changed after the last check.
    InputsChanged,
    /// The model reports a violation; an agent confirms or disputes it with a record.
    ModelFailed,
    /// The model could not decide, or the assessment could not complete.
    ModelUnresolved,
    /// Captured without a model, and no earlier assessment had the same input.
    NotAssessed,
    /// The contract entered the review's scope and has not been checked.
    NotChecked,
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "This impl sits beside the report section it serves."
)]
impl Summary {
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Counts of results, contracts and requests are bounded by the repository's files \
                  and the request budget, far below the integer's limit."
    )]
    pub(crate) fn record_conclusion(&mut self, conclusion: &ContractResult) {
        self.record_request(conclusion.request.as_ref());
        self.record_failure(conclusion.failure);
        self.cached += usize::from(conclusion.cached);
        match conclusion.status {
            ContractStatus::Verified => self.contracts_verified += 1,
            ContractStatus::Clear => self.contracts_clear += 1,
            ContractStatus::Preserved => self.contracts_preserved += 1,
            ContractStatus::Unaffected => self.contracts_unaffected += 1,
            ContractStatus::Failed => self.contracts_failed += 1,
            ContractStatus::Unresolved => self.contracts_unresolved += 1,
        }
        if conclusion.error.is_some() {
            self.errors += 1;
        }
    }
}

/// An error followed by each of its causes, joined by ": ", as results, the review log and the
/// command line's error report show it.
#[derive(Debug)]
pub struct Causes<'error>(pub &'error (dyn error::Error + 'static));

impl fmt::Display for Causes<'_> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        let mut cause = self.0.source();
        while let Some(error) = cause {
            write!(f, ": {error}")?;
            cause = error.source();
        }
        Ok(())
    }
}
