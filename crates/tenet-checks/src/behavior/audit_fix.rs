use std::{
    io,
    path::{PathBuf, absolute},
};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{AuditContractOutput, AuditReplayTarget, WitnessRepro};

/// Repair verification status reported for one saved audit witness.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditFixStatus {
    AlreadyPassing,
    Failed,
    Fixed,
    Unresolved,
}

/// One grouped module repair unit submitted to the isolated `audit-fix` worker.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditFixGroup {
    pub contracts: Vec<AuditContractOutput>,
    pub files: Vec<String>,
    pub guards: Vec<AuditReplayTarget>,
    pub name: String,
    pub related_files: Vec<String>,
    pub witnesses: Vec<AuditFixWitness>,
}

define_witness_outcome! {
    /// Verification outcome for one witness after bounded sandboxed repair.
    pub struct AuditFixOutcome { status: AuditFixStatus }
}

/// Verified replacement file returned by the isolated `audit-fix` worker.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditFixPatch {
    pub content: String,
    pub original_hash: String,
    pub path: String,
}

/// Request payload dispatched to the embedded checker worker in `audit-fix` mode.
#[derive(Clone, Debug)]
pub struct AuditFixRequest {
    pub deep: bool,
    pub groups: Vec<AuditFixGroup>,
    pub max_requests: usize,
    pub root: PathBuf,
    pub runtime_paths: Vec<String>,
    pub state: PathBuf,
}

/// Aggregate repair outcomes and verified patches returned by the isolated `audit-fix` worker.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditFixResponse {
    pub cost_usd: Option<f64>,
    pub outcomes: Vec<AuditFixOutcome>,
    pub patches: Vec<AuditFixPatch>,
    pub requests: usize,
}

/// One failing saved witness submitted for bounded repair.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditFixWitness {
    pub check: String,
    pub contract: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock_path: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repro: Option<WitnessRepro>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}

/// Repair failing saved audit witnesses in a bounded sandboxed verify-until-green loop.
#[inline]
pub fn run_audit_fix(
    request: &AuditFixRequest,
    on_progress: Option<&mut dyn FnMut(&str)>,
) -> io::Result<AuditFixResponse> {
    let input = json!({
        "mode": "audit-fix",
        "root": absolute(&request.root)?,
        "state": absolute(&request.state)?,
        "runtimePaths": request.runtime_paths,
        "maxRequests": request.max_requests,
        "deep": request.deep,
        "groups": request.groups,
    });
    let bytes = super::process::execute_bytes(&input, &request.root, true, on_progress)?;
    Ok(serde_json::from_slice(&bytes)?)
}
