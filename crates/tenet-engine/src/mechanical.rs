//! Compare evidence supplied by a trusted local checker with the current working tree.

mod inputs;
mod local;
mod receipt;

use alloc::collections::BTreeMap;
use std::{
    io,
    path::{Path, PathBuf},
};

pub use local::{LocalMode, has_local_results, inspect_local, run_local};
use serde::Serialize;
use tenet_contracts::Contract;

use self::{inputs::Inputs, receipt::ReceiptSet};
use crate::{Selection, plan};

/// What a trusted checker observed when it ran.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Fail,
    Pass,
    Unresolved,
}

/// A concrete example supplied by a structured project checker.
#[derive(Clone, Debug, serde::Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub actual: String,
    pub case: String,
    pub expected: String,
    pub location: Option<String>,
}

/// Name in a local check result or in the version 1 imported receipt format.
#[derive(Debug, Serialize)]
pub enum CheckName {
    #[serde(rename = "obligation")]
    Imported(String),
    #[serde(rename = "check")]
    Local(String),
}

impl CheckName {
    /// The project-declared identifier in either result format.
    #[must_use]
    #[inline]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Imported(name) | Self::Local(name) => name,
        }
    }
}

/// One declared check and how its evidence relates to the current inputs.
#[derive(Debug, Serialize)]
pub struct Evidence {
    pub checker: Option<String>,
    pub contract: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finding: Option<Finding>,
    pub freshness: Freshness,
    pub message: Option<String>,
    #[serde(flatten)]
    pub name: CheckName,
    pub observed_outcome: Option<Outcome>,
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replay: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reused: Option<bool>,
}

/// Whether an observation can be used for this working tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Current,
    Missing,
    Stale,
    Unresolved,
}

/// Read a caller-selected evidence file and report all declared obligations, without running
/// the checker or writing review state. The caller supplies the file from a trusted runner.
#[inline]
pub fn verify(root: &Path, receipt_path: &Path) -> Result<Vec<Evidence>, ReceiptError> {
    let receipts = ReceiptSet::read(receipt_path)?;
    let plan = plan(root, &Selection::default())?;
    let contracts: BTreeMap<&str, &Contract> = plan
        .contracts
        .iter()
        .map(|contract| (contract.name.as_str(), contract))
        .collect();
    receipts.verify_names(&contracts)?;
    let mut inputs = Inputs::new(&plan.root, &plan.inventory);
    let mut evidence = Vec::new();
    for contract in &plan.contracts {
        for obligation in &contract.obligations {
            evidence.push(receipts.report(contract, obligation, &mut inputs));
        }
    }
    Ok(evidence)
}

/// Why Tenet could not read the supplied evidence as a receipt set.
#[derive(Debug, thiserror::Error)]
pub enum ReceiptError {
    #[error(transparent)]
    Discovery(#[from] crate::DiscoveryError),
    #[error("invalid mechanical evidence: {reason:#}")]
    Invalid { reason: String },
    #[error("cannot decode mechanical evidence {path:?}: {source:#}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("cannot access local check state {path:?}: {source:#}")]
    Local { path: PathBuf, source: io::Error },
    #[error("cannot read mechanical evidence {path:?}: {source:#}")]
    Read { path: PathBuf, source: io::Error },
}
