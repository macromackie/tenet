use std::{
    io,
    path::{PathBuf, absolute},
};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Outcome, WitnessRepro};

/// Request payload dispatched to the embedded checker worker in zero-model `replay` mode.
#[derive(Clone, Debug)]
pub struct AuditReplayRequest {
    pub root: PathBuf,
    pub runtime_paths: Vec<String>,
    pub state: PathBuf,
    pub targets: Vec<AuditReplayTarget>,
}

/// Aggregate response returned by the isolated checker worker in `replay` mode.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditReplayResponse {
    pub results: Vec<AuditReplayResult>,
}

define_witness_outcome! {
    /// Outcome of replaying one saved audit witness against the current checkout.
    pub struct AuditReplayResult { outcome: Outcome }
}

/// Saved audit witness target submitted to the isolated checker worker for zero-model replay.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditReplayTarget {
    pub check: String,
    pub contract: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repro: Option<WitnessRepro>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}

/// Replay saved audit witnesses against the current checkout without calling a model.
#[inline]
pub fn replay_audit(
    request: &AuditReplayRequest,
    on_progress: Option<&mut dyn FnMut(&str)>,
) -> io::Result<AuditReplayResponse> {
    let input = json!({
        "mode": "replay",
        "root": request.root,
        "state": absolute(&request.state)?,
        "runtimePaths": request.runtime_paths,
        "targets": request.targets,
    });
    let bytes = super::process::execute_bytes(&input, &request.root, false, on_progress)?;
    Ok(serde_json::from_slice(&bytes)?)
}
