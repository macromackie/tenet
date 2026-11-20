//! Native rule observations tied to the source input and the adopted configuration.

use alloc::collections::BTreeMap;
use std::{
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tenet_checks::{
    behavior::{Finding, Outcome},
    fixes,
};
use tenet_contracts::Contract;
use tenet_engine::{Mode, Plan, review::Checked};

use crate::error::CommandError;

const DIRECTORY: &str = "native-checks";

fn find_receipt_path(state: &Path, contract: &str, input: &str) -> PathBuf {
    let mut hash = Sha256::new();
    hash.update(env!("CARGO_PKG_VERSION"));
    hash.update(contract);
    hash.update(input);
    state
        .join(DIRECTORY)
        .join(format!("{:x}.json", hash.finalize()))
}

#[derive(Deserialize, Serialize)]
struct Receipt {
    assets: String,
    rows: Vec<Finding>,
}

fn hash_assets(root: &Path, contract: &Contract) -> io::Result<String> {
    let mut hash = Sha256::new();
    let document = root.join(&contract.path);
    for path in [
        root.join(".tenet.lock"),
        document
            .with_file_name("assets")
            .join("oxlint/oxlintrc.json"),
        document.with_file_name("assets").join("ruff/ruff.toml"),
    ] {
        match fs::symlink_metadata(&path) {
            Ok(info) if info.is_file() && info.len() <= 0x0040_0000 => {
                hash.update(Sha256::digest(fs::read(path)?));
            },
            Ok(_) => hash.update(b"unreadable"),
            Err(error) if error.kind() == io::ErrorKind::NotFound => hash.update(b"missing"),
            Err(error) => return Err(error),
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub(super) fn read(
    root: &Path,
    state: &Path,
    contract: &Contract,
    input: &str,
) -> Result<Vec<Finding>, CommandError> {
    let path = find_receipt_path(state, &contract.name, input);
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut saved: Receipt = serde_json::from_slice(&bytes)?;
    if saved.assets != hash_assets(root, contract)? {
        for row in &mut saved.rows {
            row.outcome = Outcome::Unresolved;
            row.message = "Native rule settings changed. Run tenet check again.".into();
        }
    }
    Ok(saved.rows)
}

fn save(
    plan: &Plan,
    state: &Path,
    checked: &Checked,
    rows: &[Finding],
) -> Result<(), CommandError> {
    let mut groups = BTreeMap::<_, Vec<_>>::new();
    for row in rows {
        groups
            .entry(row.contract.as_str())
            .or_default()
            .push(row.clone());
    }
    if groups.is_empty() {
        return Ok(());
    }
    let directory = state.join(DIRECTORY);
    fs::create_dir_all(&directory)?;
    if fs::symlink_metadata(&directory)?.is_symlink() {
        return Err(io::Error::other("native review directory is a symbolic link").into());
    }
    for entry in &checked.contracts {
        if let Some(group) = groups.remove(entry.contract.as_str())
            && let Some(contract) = plan
                .contracts
                .iter()
                .find(|contract| contract.name == entry.contract)
        {
            let mut file = tempfile::NamedTempFile::new_in(&directory)?;
            file.write_all(&serde_json::to_vec(&Receipt {
                assets: hash_assets(&plan.root, contract)?,
                rows: group,
            })?)?;
            drop(
                file.persist(find_receipt_path(state, &entry.contract, &entry.input))
                    .map_err(|error| error.error)?,
            );
        }
    }
    Ok(())
}

fn build_finding(contract: String, tool: &str, outcome: Outcome, message: String) -> Finding {
    Finding {
        check: format!("native-{tool}"),
        contract,
        cost_usd: None,
        generation_requests: None,
        message,
        outcome,
        requested: false,
        reused: false,
        subject: None,
    }
}

pub(super) async fn assess(
    plan: &Plan,
    state: &Path,
    checked: &Checked,
) -> Result<Vec<Finding>, CommandError> {
    let report = fixes::inspect(plan).await.map_err(anyhow::Error::from)?;
    let mut rows = Vec::new();
    for item in report.verified {
        let (outcome, text) = match (item.passed, plan.mode) {
            (true, _) => {
                (
                    Outcome::Passed,
                    format!("{} found no violations of its selected rules.", item.tool),
                )
            },
            (false, Mode::Full) => {
                (
                    Outcome::Failed,
                    item.finding
                        .unwrap_or_else(|| format!("{} reported a violation.", item.tool)),
                )
            },
            (false, Mode::Diff) => {
                let message = item
                    .finding
                    .unwrap_or_else(|| format!("{} reported a violation.", item.tool));
                (
                    Outcome::Unresolved,
                    format!("{message} Check whether the same finding exists on the base."),
                )
            },
        };
        rows.push(build_finding(item.contract, item.tool, outcome, text));
    }
    for item in report.unresolved {
        rows.push(build_finding(
            item.contract,
            item.tool,
            Outcome::Unresolved,
            item.message,
        ));
    }
    save(plan, state, checked, &rows)?;
    Ok(rows)
}
