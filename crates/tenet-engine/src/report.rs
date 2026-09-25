use serde::Serialize;
use tenet_contracts::Verdict;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Applicability,
    Verification,
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

#[derive(Debug, Serialize)]
pub struct CheckResult {
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
        } else if self.uncertain > 0 {
            3
        } else {
            u8::from(self.failed > 0)
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
    pub broadened: bool,
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
