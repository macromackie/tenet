use alloc::collections::BTreeMap;
use std::{
    fs::File,
    io::Read as _,
    path::{Component, Path, PathBuf},
};

use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use tenet_contracts::Contract;
use tenet_engine::is_allowed;

use super::{CheckName, Evidence, Freshness, Outcome, ReceiptError, inputs::Inputs};

const MAX_RECEIPT_BYTES: u64 = 2 * 1024 * 1024;
const RECEIPT_VERSION: u32 = 1;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptDocument {
    receipts: Vec<Receipt>,
    version: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    checker: String,
    contract: String,
    contract_sha256: String,
    discover: Vec<String>,
    files: BTreeMap<PathBuf, String>,
    message: String,
    obligation: String,
    outcome: Outcome,
}

/// Immutable, validated observations read from a caller-selected trusted runner.
#[derive(Debug)]
pub struct ReceiptSet(BTreeMap<(String, String), Receipt>);

const fn reject(reason: String) -> ReceiptError {
    ReceiptError::Invalid { reason }
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_safe_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        && is_allowed(path)
}

impl ReceiptSet {
    #[expect(
        clippy::unnecessary_debug_formatting,
        reason = "Quote receipt paths and escape control characters before printing them."
    )]
    pub(super) fn read(path: &Path) -> Result<Self, ReceiptError> {
        let mut bytes = Vec::new();
        if !path.is_file() {
            return Err(reject(format!(
                "receipt set {:?} must be a regular file",
                path.as_os_str()
            )));
        }
        let _read_bytes = File::open(path)
            .and_then(|handle| handle.take(MAX_RECEIPT_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|source| {
                ReceiptError::Read {
                    path: path.to_owned(),
                    source,
                }
            })?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_RECEIPT_BYTES {
            return Err(reject(format!(
                "receipt set exceeds {MAX_RECEIPT_BYTES:?} bytes"
            )));
        }
        let document: ReceiptDocument = serde_json::from_slice(&bytes).map_err(|source| {
            ReceiptError::Json {
                path: path.to_owned(),
                source,
            }
        })?;
        if document.version != RECEIPT_VERSION {
            return Err(reject(format!(
                "unsupported version {:?}",
                document.version
            )));
        }
        let mut receipts = BTreeMap::new();
        for receipt in document.receipts {
            receipt.validate()?;
            let key = (receipt.contract.clone(), receipt.obligation.clone());
            if receipts.insert(key.clone(), receipt).is_some() {
                return Err(reject(format!("duplicate receipt for {key:?}")));
            }
        }
        Ok(Self(receipts))
    }

    pub(super) fn report(
        &self,
        contract: &Contract,
        obligation: &str,
        inputs: &mut Inputs<'_>,
    ) -> Evidence {
        let mut evidence = Evidence {
            checker: None,
            contract: contract.name.clone(),
            finding: None,
            freshness: Freshness::Missing,
            message: None,
            name: CheckName::Imported(obligation.to_owned()),
            observed_outcome: None,
            reason: None,
            replay: None,
            reused: None,
        };
        if let Some(receipt) = self.0.get(&(contract.name.clone(), obligation.to_owned())) {
            evidence.checker = Some(receipt.checker.clone());
            evidence.message = Some(receipt.message.clone());
            evidence.observed_outcome = Some(receipt.outcome);
            (evidence.freshness, evidence.reason) = receipt.assess(contract, inputs);
        }
        evidence
    }

    pub(super) fn verify_names(
        &self,
        contracts: &BTreeMap<&str, &Contract>,
    ) -> Result<(), ReceiptError> {
        for (contract, obligation) in self.0.keys() {
            if !contracts
                .get(contract.as_str())
                .is_some_and(|found| found.obligations.contains(obligation))
            {
                return Err(reject(format!(
                    "unknown obligation {contract:?}/{obligation:?}"
                )));
            }
        }
        Ok(())
    }
}

impl Receipt {
    fn assess(&self, contract: &Contract, inputs: &mut Inputs<'_>) -> (Freshness, Option<String>) {
        if self.contract_sha256 != format!("{:x}", Sha256::digest(contract.document.as_bytes())) {
            return (Freshness::Stale, Some("contract bytes changed".to_owned()));
        }
        if !self.files.keys().any(|path| contract.is_governing(path)) {
            return (
                Freshness::Unresolved,
                Some("receipt names no governed subject".to_owned()),
            );
        }
        inputs.assess(&self.files, &self.discover)
    }

    fn validate(&self) -> Result<(), ReceiptError> {
        if !is_sha256(&self.contract_sha256)
            || self
                .files
                .iter()
                .any(|(path, hash)| !is_safe_path(path) || !is_sha256(hash))
            || self
                .discover
                .iter()
                .any(|glob| !is_safe_path(Path::new(glob)))
        {
            return Err(reject(format!(
                "invalid input path or SHA-256 for {:?}",
                self.obligation
            )));
        }
        if self.checker.trim().is_empty() || self.message.trim().is_empty() {
            return Err(reject(format!(
                "missing checker or message for {:?}",
                self.obligation
            )));
        }
        Ok(())
    }
}
