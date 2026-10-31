use std::path::PathBuf;

use serde::Serialize;

use super::{CheckResult, ContractResult, ContractState, Stage, Summary};

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
