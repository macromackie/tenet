use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Applicability,
    Verification,
    Completeness,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pass,
    Fail,
    NotApplicable,
    Uncertain,
    Error,
    Incomplete,
    /// No text to judge: an empty, binary, non-UTF-8, or symbolic-link file.
    Skipped,
}

/// Part of a file's change left out of its model input to fit the input limit.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Omitted {
    /// The prior contents; the patch and resulting contents remain.
    Before,
    /// The resulting contents too; only the patch remains.
    Contents,
}

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
    pub applicability: Option<ev_grep_core::Assessment>,
    pub applicability_routing: Option<ev_grep_core::Routing>,
    pub verification: Option<ev_grep_core::Assessment>,
    pub verification_routing: Option<ev_grep_core::Routing>,
    pub request: Option<ev_grep_core::RequestInfo>,
    /// The answers came from the review's answer cache; no request was sent.
    pub cached: bool,
    pub elapsed_ms: u128,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Summary {
    pub passed: usize,
    pub failed: usize,
    pub uncertain: usize,
    pub not_applicable: usize,
    pub errors: usize,
    pub incomplete: usize,
    pub skipped: usize,
    pub requests: usize,
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
    pub(crate) fn merge(&mut self, other: &Self) {
        self.passed += other.passed;
        self.failed += other.failed;
        self.uncertain += other.uncertain;
        self.not_applicable += other.not_applicable;
        self.errors += other.errors;
        self.incomplete += other.incomplete;
        self.skipped += other.skipped;
        self.requests += other.requests;
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

    fn record_request(&mut self, request: Option<&ev_grep_core::RequestInfo>) {
        if let Some(request) = request {
            self.attempts += request.attempts;
            self.hedges += request.hedges;
            self.input_tokens += request.input_tokens;
            self.output_tokens += request.output_tokens;
        }
    }

    pub(crate) fn add(&mut self, result: &CheckResult) {
        self.record_request(result.request.as_ref());
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
}

#[derive(Serialize)]
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
    pub deleted_contracts: Vec<std::path::PathBuf>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Begin {
        run: RunInfo,
    },
    ContractStarted {
        contract: String,
        path: String,
        total: usize,
    },
    CheckStarted {
        contract: String,
        path: String,
    },
    StageStarted {
        contract: String,
        path: String,
        stage: Stage,
    },
    StageCompleted {
        contract: String,
        path: String,
        stage: Stage,
        outcome: ev_grep_core::Outcome,
    },
    Result {
        result: Box<CheckResult>,
    },
    ContractFinished {
        contract: String,
        summary: Summary,
        conclusion: Option<Box<ContractResult>>,
        state: ContractState,
    },
    Skipped {
        path: String,
        reason: String,
    },
    Summary {
        summary: Summary,
        /// The revision this check appended to `.tenet/events.jsonl`; absent when nothing is kept.
        revision: Option<u64>,
        /// Contracts that need a check or a record before the review is final.
        pending: Vec<String>,
        exit_code: u8,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractStatus {
    Verified,
    Preserved,
    Clear,
    Unaffected,
    Failed,
    Unresolved,
}

impl ContractStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Preserved => "preserved",
            Self::Clear => "clear",
            Self::Unaffected => "unaffected",
            Self::Failed => "failed",
            Self::Unresolved => "unresolved",
        }
    }

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
pub fn violation(assessment: &ev_grep_core::Assessment) -> f64 {
    assessment
        .probabilities
        .get(&ev_grep_core::Outcome::Match)
        .copied()
        .unwrap_or(0.0)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestedScope {
    FullContract,
    SelectedSubjects,
}

impl RequestedScope {
    /// What a conclusion about this scope covers, in prose.
    pub fn description(self) -> &'static str {
        match self {
            Self::FullContract => "the full contract",
            Self::SelectedSubjects => "selected subjects",
        }
    }
}

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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub conflicting_files: Vec<String>,
    pub request: Option<ev_grep_core::RequestInfo>,
    /// The combined answer came from the answer cache, or `check --no-model` reused an earlier
    /// check's conclusion for the same input; no request was sent.
    pub cached: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConclusionReason {
    FileChecksIncomplete,
    NoRelevantChanges,
    RequestBudget,
    ContextTooLarge,
    ContextUnavailable,
    AssessmentFailed,
    Violation,
    /// File checks found a violation the combined assessment did not confirm or could not run.
    FileViolation,
    Compliance,
    SelectedSubjectsClear,
    ModelUncertain,
    ConflictingAssessments,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContextSummary {
    pub input_bytes: usize,
    pub limit_bytes: usize,
    pub included_files: usize,
    pub selected_files: Vec<std::path::PathBuf>,
    pub support_files: Vec<std::path::PathBuf>,
    /// Scoped inventory files whose contents were not supplied.
    pub omitted_files: usize,
    /// Prior contents left out of every change to fit the limit; patches remain.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub before_omitted: bool,
    /// Inventory paths summarized as per-directory file counts to fit the limit.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub inventory_counted: bool,
    /// Changed files whose resulting contents were left out to fit the limit; patches remain.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub contents_omitted: Vec<std::path::PathBuf>,
}

impl ContextSummary {
    /// Whether the combined input fits its limit, so the combined assessment can run.
    pub fn fits(&self) -> bool {
        self.input_bytes <= self.limit_bytes
    }

    /// Whether anything was left out of a combined input that fit the limit.
    pub fn is_reduced(&self) -> bool {
        self.fits()
            && (self.before_omitted || self.inventory_counted || !self.contents_omitted.is_empty())
    }
}

/// Where a contract stands in its review once records and model conclusions are combined.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ContractState {
    /// A favorable model conclusion, or a record bound to the current input.
    Settled {
        conclusion: ContractStatus,
        /// The record that settled the contract; absent when the model's conclusion did.
        record: Option<String>,
    },
    /// The result is not final until a check or a record settles it.
    Pending { reason: Pending },
}

impl ContractState {
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending { .. })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Pending {
    /// The model reports a violation; an agent confirms or disputes it with a record.
    ModelFailed,
    /// The model could not decide, or the assessment could not complete.
    ModelUnresolved,
    /// Captured without a model, and no earlier assessment had the same input.
    NotAssessed,
    /// Scoped files or the contract changed after the last check.
    InputsChanged,
    /// The contract entered the review's scope and has not been checked.
    NotChecked,
    /// The diff adds or edits the contract itself, so a diff check's favorable conclusion, which
    /// assumes the base complied, does not settle it; a full check informs the record that does.
    ContractChanged,
}

impl Summary {
    pub(crate) fn conclude(&mut self, conclusion: &ContractResult) {
        self.record_request(conclusion.request.as_ref());
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
