use serde::{Deserialize, Serialize};
use tenet_contracts::Verdict;

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

impl From<Verdict> for Status {
    fn from(v: Verdict) -> Self {
        match v {
            Verdict::Pass => Self::Pass,
            Verdict::Fail => Self::Fail,
            Verdict::NotApplicable => Self::NotApplicable,
            Verdict::Uncertain => Self::Uncertain,
        }
    }
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
    pub example: Option<String>,
    pub expected: Option<Verdict>,
    pub status: Status,
    pub message: String,
    pub reason: Option<String>,
    pub applicability: Option<ev_grep_core::Assessment>,
    pub verification: Option<ev_grep_core::Assessment>,
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
    pub examples_passed: usize,
    pub examples_failed: usize,
    pub requests: usize,
    pub elapsed_ms: u128,
    pub cancelled: bool,
    pub contracts_verified: usize,
    pub contracts_preserved: usize,
    pub contracts_unaffected: usize,
    pub contracts_failed: usize,
    pub contracts_unresolved: usize,
}

impl Summary {
    pub(crate) fn add(&mut self, result: &CheckResult) {
        match result.status {
            Status::Pass => self.passed += 1,
            Status::Fail => self.failed += 1,
            Status::Uncertain => self.uncertain += 1,
            Status::NotApplicable => self.not_applicable += 1,
            Status::Error => self.errors += 1,
            Status::Incomplete => self.incomplete += 1,
        }
        if let Some(expected) = result.expected {
            if result.status == Status::from(expected) {
                self.examples_passed += 1;
            } else {
                self.examples_failed += 1;
            }
        }
    }

    pub fn exit_code(&self, evaluation: bool) -> u8 {
        if self.cancelled {
            130
        } else if self.errors > 0 {
            2
        } else if self.incomplete > 0 {
            3
        } else if evaluation {
            u8::from(self.examples_failed > 0)
        } else {
            u8::from(self.failed > 0 || self.contracts_failed > 0)
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
        conclusion: Option<ContractResult>,
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
    pub status: ContractStatus,
    pub reason: String,
    pub assessment: Option<ev_grep_core::Assessment>,
    pub evidence_hash: Option<String>,
    pub error: Option<String>,
}

impl Summary {
    pub(crate) fn conclude(&mut self, conclusion: &ContractResult) {
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
