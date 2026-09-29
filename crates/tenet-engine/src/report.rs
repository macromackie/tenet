use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Applicability,
    Verification,
    Completeness,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pass,
    Fail,
    NotApplicable,
    Uncertain,
    Error,
    Incomplete,
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
    pub source_hash: Option<String>,
    pub status: Status,
    pub message: String,
    pub reason: Option<String>,
    pub applicability: Option<ev_grep_core::Assessment>,
    pub verification: Option<ev_grep_core::Assessment>,
    pub request: Option<ev_grep_core::RequestInfo>,
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
    pub requests: usize,
    pub attempts: usize,
    pub hedges: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub elapsed_ms: u128,
    pub cancelled: bool,
    pub contracts_verified: usize,
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
        self.requests += other.requests;
        self.attempts += other.attempts;
        self.hedges += other.hedges;
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.contracts_verified += other.contracts_verified;
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
        match result.status {
            Status::Pass => self.passed += 1,
            Status::Fail => self.failed += 1,
            Status::Uncertain => self.uncertain += 1,
            Status::NotApplicable => self.not_applicable += 1,
            Status::Error => self.errors += 1,
            Status::Incomplete => self.incomplete += 1,
        }
    }

    pub fn exit_code(&self) -> u8 {
        if self.cancelled {
            130
        } else if self.errors > 0 {
            2
        } else if self.incomplete > 0 {
            3
        } else {
            u8::from(self.contracts_failed > 0)
        }
    }
}

#[derive(Serialize)]
pub struct RunInfo {
    pub version: String,
    pub root: String,
    pub provider: String,
    pub model: String,
    pub mode: String,
    pub base: Option<String>,
    pub head: Option<String>,
    pub assumption: Option<String>,
    pub partial: bool,
    pub snapshot: Option<String>,
    pub patch_hash: Option<String>,
    pub jobs: usize,
    pub max_requests: usize,
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
    },
    Skipped {
        path: String,
        reason: String,
    },
    Summary {
        summary: Summary,
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
    Unaffected,
    Failed,
    Unresolved,
}

#[derive(Debug, Serialize)]
pub struct ContractResult {
    #[serde(rename = "assessment")]
    pub status: ContractStatus,
    pub confidence: Option<f64>,
    #[serde(skip)]
    pub reason: String,
    #[serde(skip)]
    pub assessment: Option<ev_grep_core::Assessment>,
    #[serde(skip)]
    pub evidence_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub conflicting_files: Vec<String>,
    pub request: Option<ev_grep_core::RequestInfo>,
}

impl Summary {
    pub(crate) fn conclude(&mut self, conclusion: &ContractResult) {
        self.record_request(conclusion.request.as_ref());
        match conclusion.status {
            ContractStatus::Verified => self.contracts_verified += 1,
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
