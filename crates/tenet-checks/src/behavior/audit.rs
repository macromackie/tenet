use std::{
    io,
    path::{PathBuf, absolute},
};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::Finding;

macro_rules! define_witness_struct {
    (
        $(#[$meta:meta])*
        pub struct $name:ident {
            $($(#[$field_meta:meta])* $field:ident : $field_ty:ty),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            pub check: String,
            pub contract: String,
            $($(#[$field_meta])* pub $field: $field_ty,)*
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub subject: Option<String>,
        }
    };
}

macro_rules! define_witness_outcome {
    ($(#[$meta:meta])* pub struct $name:ident { $field:ident : $status_ty:ty }) => {
        define_witness_struct! {
            $(#[$meta])*
            pub struct $name {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                expected: Option<String>,
                message: String,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                observed: Option<String>,
                $field: $status_ty,
            }
        }
    };
}

macro_rules! define_witness_target {
    ($(#[$meta:meta])* pub struct $name:ident { $($extra_field:ident : $extra_ty:ty),* $(,)? }) => {
        define_witness_struct! {
            $(#[$meta])*
            pub struct $name {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                lock_path: Option<String>,
                $($extra_field: $extra_ty,)*
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pin_root: Option<String>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                repro: Option<WitnessRepro>,
            }
        }
    };
}

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

/// One inclusive 1-based line window inside an oversized production source file.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditFileWindow {
    pub end_line: usize,
    pub path: String,
    pub start_line: usize,
}

/// One repository module shard submitted to the isolated audit worker.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditModuleInput {
    #[serde(default)]
    pub changed_files: Vec<String>,
    pub docs: Vec<String>,
    pub files: Vec<String>,
    #[serde(default)]
    pub low_test_files: Vec<String>,
    pub manifests: Vec<String>,
    pub name: String,
    #[serde(default)]
    pub related_files: Vec<String>,
    #[serde(default)]
    pub test_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<AuditFileWindow>,
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
        "root": absolute(&request.root)?,
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
