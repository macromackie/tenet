use std::{
    io,
    path::{PathBuf, absolute},
};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::Finding;

/// Synthesized contract document and staged pin path returned by one module audit.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditContractOutput {
    pub document: String,
    pub lock_path: String,
    pub message: String,
    pub name: String,
    pub paths: Vec<String>,
}

/// One repository module shard submitted to the isolated audit worker.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditModuleInput {
    pub docs: Vec<String>,
    pub files: Vec<String>,
    #[serde(default)]
    pub low_test_files: Vec<String>,
    pub manifests: Vec<String>,
    pub name: String,
}

/// Verified findings and synthesized contracts for one audited module.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditModuleReport {
    pub contracts: Vec<AuditContractOutput>,
    pub cost_usd: Option<f64>,
    pub findings: Vec<Finding>,
    pub name: String,
    pub requests: usize,
}

/// Request payload dispatched to the embedded checker worker in `audit` mode.
#[derive(Clone, Debug)]
pub struct AuditRequest {
    pub deep: bool,
    pub fresh: bool,
    pub max_requests: usize,
    pub modules: Vec<AuditModuleInput>,
    pub root: PathBuf,
    pub runtime_paths: Vec<String>,
    pub state: PathBuf,
}

/// Aggregate response returned by the isolated audit worker.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditResponse {
    pub cost_usd: Option<f64>,
    pub modules: Vec<AuditModuleReport>,
    pub requests: usize,
}

/// Run bounded module synthesis and sandboxed falsification through the embedded checker worker.
///
/// Fails with an `io::Error` if the worker cannot start, exceeds its time or output bounds, or
/// returns malformed JSON.
#[inline]
pub fn run_audit(
    request: &AuditRequest,
    on_progress: Option<&mut dyn FnMut(&str)>,
) -> io::Result<AuditResponse> {
    let input = json!({
        "mode": "audit",
        "root": request.root,
        "state": absolute(&request.state)?,
        "modules": request.modules,
        "runtimePaths": request.runtime_paths,
        "deep": request.deep,
        "fresh": request.fresh,
        "maxRequests": request.max_requests,
    });
    let bytes = super::process::execute_bytes(&input, &request.root, true, on_progress)?;
    Ok(serde_json::from_slice(&bytes)?)
}
