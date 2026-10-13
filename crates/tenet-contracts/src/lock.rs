use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::parse::{conflict_marker, is_kebab, with_name};

/// File name of the committed adoption record at the project root.
pub const LOCK_FILE: &str = ".tenet.lock";
/// Alias of the public library, always written so a lock stands on its own.
pub const DEFAULT_SOURCE: &str = "tenet";
pub const DEFAULT_SOURCE_URL: &str = "https://github.com/macromackie/tenet-contracts";

/// Contracts installed from shared sources, keyed by their local `CONTRACT.md` path.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Lock {
    pub version: u32,
    pub sources: BTreeMap<String, String>,
    pub contracts: BTreeMap<PathBuf, Entry>,
}

/// One installed contract and the upstream revision that is its next merge base.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub source: String,
    pub contract: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub commit: String,
    pub hash: String,
}

impl Default for Lock {
    fn default() -> Self {
        Self {
            version: 1,
            sources: BTreeMap::from([(DEFAULT_SOURCE.into(), DEFAULT_SOURCE_URL.into())]),
            contracts: BTreeMap::new(),
        }
    }
}

impl Lock {
    pub fn parse(text: &str) -> Result<Self> {
        if let Some(line) = conflict_marker(text) {
            bail!("{LOCK_FILE}:{line}: unresolved merge conflict marker");
        }
        let lock: Self =
            serde_json::from_str(text).with_context(|| format!("invalid {LOCK_FILE}"))?;
        ensure!(
            lock.version == 1,
            "unsupported {LOCK_FILE} version {}",
            lock.version
        );
        for (alias, url) in &lock.sources {
            ensure!(is_kebab(alias), "{LOCK_FILE}: invalid source alias {alias}");
            ensure!(
                !url.is_empty() && !url.starts_with('-'),
                "{LOCK_FILE}: invalid URL for source {alias}"
            );
        }
        for (path, entry) in &lock.contracts {
            ensure!(
                is_contract_path(path),
                "{LOCK_FILE}: {} is not a relative .contracts/NNN-name/CONTRACT.md path",
                path.display()
            );
            ensure!(
                lock.sources.contains_key(&entry.source),
                "{LOCK_FILE}: {} names unknown source {}",
                path.display(),
                entry.source
            );
            ensure!(
                is_kebab(&entry.contract),
                "{LOCK_FILE}: {} has an invalid contract name",
                path.display()
            );
            ensure!(
                entry.commit.len() == 40 && entry.commit.bytes().all(|b| b.is_ascii_hexdigit()),
                "{LOCK_FILE}: {} has an invalid commit",
                path.display()
            );
            ensure!(
                entry.hash.strip_prefix("blake3:").is_some_and(
                    |hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())
                ),
                "{LOCK_FILE}: {} has an invalid hash",
                path.display()
            );
        }
        Ok(lock)
    }

    /// Sorted, pretty JSON with a trailing newline, so independent additions touch separate lines.
    pub fn render(&self) -> Result<String> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        Ok(text)
    }
}

/// Hash a contract document without its `name` value, so a locally renamed copy still matches upstream.
pub fn content_hash(document: &str) -> Result<String> {
    Ok(format!(
        "blake3:{}",
        blake3::hash(with_name(document, "")?.as_bytes()).to_hex()
    ))
}

fn is_contract_path(path: &Path) -> bool {
    let parts: Vec<_> = path.components().collect();
    parts
        .iter()
        .all(|part| matches!(part, Component::Normal(_)))
        && parts.len() >= 3
        && parts[parts.len() - 1].as_os_str() == "CONTRACT.md"
        && parts[parts.len() - 3].as_os_str() == ".contracts"
}
