//! Compare evidence supplied by a trusted local checker with the current working tree.

mod inputs;
mod local;
mod receipt;

use alloc::collections::BTreeMap;
use std::{
    io,
    path::{Path, PathBuf},
};

pub use local::{
    LocalMode,
    Qualification,
    QualificationCase,
    has_local_results,
    inspect_local,
    inspect_selected_local,
    qualify,
    run_local,
    run_selected_local,
};
pub use receipt::ReceiptSet;
use serde::Serialize;
use tenet_contracts::Contract;

use self::inputs::Inputs;
use crate::{Plan, Selection, plan};

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
    receipts.validate_contracts(&plan)?;
    Ok(receipts.verify_plan(&plan))
}

impl ReceiptSet {
    /// Load the trusted runner's bytes and validate its named checks against the whole project.
    /// Call before a paid run so an invalid receipt file cannot incur model spend.
    #[inline]
    pub fn open(root: &Path, path: &Path) -> Result<Self, ReceiptError> {
        let receipts = Self::read(path)?;
        receipts.validate_contracts(&plan(root, &Selection::default())?)?;
        Ok(receipts)
    }

    fn validate_contracts(&self, plan: &Plan) -> Result<(), ReceiptError> {
        let contracts: BTreeMap<&str, &Contract> = plan
            .contracts
            .iter()
            .map(|contract| (contract.name.as_str(), contract))
            .collect();
        self.verify_names(&contracts)
    }

    /// Compare the loaded observations against just the selected declarations and current inputs.
    #[must_use]
    #[inline]
    pub fn verify_plan(&self, plan: &Plan) -> Vec<Evidence> {
        let mut inputs = Inputs::new(&plan.root, &plan.inventory);
        let mut evidence = Vec::new();
        for contract in &plan.contracts {
            for obligation in &contract.obligations {
                evidence.push(self.report(contract, obligation, &mut inputs));
            }
        }
        evidence
    }
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
