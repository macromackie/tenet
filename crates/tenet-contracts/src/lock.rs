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

/// The newest lock format this Tenet reads. Version 2 adds locked `files` and `skills`.
const NEWEST_VERSION: u32 = 2;

/// Contracts and skills installed from shared sources.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Lock {
    /// Read before the rest of the file; [`Lock::render`] writes the lowest version that holds
    /// the content, so a lock without files or skills stays version 1.
    version: u32,
    pub sources: BTreeMap<String, String>,
    /// Keyed by local `CONTRACT.md` path.
    pub contracts: BTreeMap<PathBuf, Entry>,
    /// Keyed by installed skill folder.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub skills: BTreeMap<PathBuf, SkillEntry>,
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
    /// Hashes of the installed assets, keyed by path relative to the contract folder, such as
    /// `assets/oxlint/rules.json`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<PathBuf, String>,
}

/// One installed skill folder and the upstream revision that is its next merge base.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkillEntry {
    pub source: String,
    pub skill: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub commit: String,
    /// Hashes of every installed file, `SKILL.md` included, keyed by path relative to the folder.
    pub files: BTreeMap<PathBuf, String>,
}

#[derive(Deserialize)]
struct Header {
    version: u32,
}

impl Default for Lock {
    fn default() -> Self {
        Self {
            version: 1,
            sources: BTreeMap::from([(DEFAULT_SOURCE.into(), DEFAULT_SOURCE_URL.into())]),
            contracts: BTreeMap::new(),
            skills: BTreeMap::new(),
        }
    }
}

impl Lock {
    pub fn parse(text: &str) -> Result<Self> {
        if let Some(line) = conflict_marker(text) {
            bail!("{LOCK_FILE}:{line}: unresolved merge conflict marker");
        }
        let Header { version } =
            serde_json::from_str(text).with_context(|| format!("invalid {LOCK_FILE}"))?;
        ensure!(
            version <= NEWEST_VERSION,
            "{LOCK_FILE} version {version} needs a Tenet newer than {}",
            env!("CARGO_PKG_VERSION")
        );
        ensure!(version >= 1, "unsupported {LOCK_FILE} version {version}");
        let lock: Self =
            serde_json::from_str(text).with_context(|| format!("invalid {LOCK_FILE}"))?;
        ensure!(
            lock.lowest_version() <= version,
            "{LOCK_FILE} version {version} cannot record files or skills; they need version 2"
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
            lock.check_origin(path, &entry.source, &entry.contract, &entry.commit)?;
            ensure!(
                is_hash(&entry.hash),
                "{LOCK_FILE}: {} has an invalid hash",
                path.display()
            );
            for (file, hash) in &entry.files {
                ensure!(
                    is_contract_asset(file),
                    "{LOCK_FILE}: {} lists {}, which is not a path under assets/<tool>/",
                    path.display(),
                    file.display()
                );
                ensure!(
                    is_hash(hash),
                    "{LOCK_FILE}: {} has an invalid hash for {}",
                    path.display(),
                    file.display()
                );
            }
        }
        for (folder, entry) in &lock.skills {
            ensure!(
                is_relative(folder),
                "{LOCK_FILE}: skill folder {} is not a relative path",
                folder.display()
            );
            lock.check_origin(folder, &entry.source, &entry.skill, &entry.commit)?;
            ensure!(
                entry.files.contains_key(Path::new("SKILL.md")),
                "{LOCK_FILE}: {} does not list SKILL.md",
                folder.display()
            );
            for (file, hash) in &entry.files {
                ensure!(
                    is_relative(file),
                    "{LOCK_FILE}: {} lists {}, which is not a relative path",
                    folder.display(),
                    file.display()
                );
                ensure!(
                    is_hash(hash),
                    "{LOCK_FILE}: {} has an invalid hash for {}",
                    folder.display(),
                    file.display()
                );
            }
        }
        Ok(lock)
    }

    /// Sorted, pretty JSON with a trailing newline, so independent additions touch separate lines.
    pub fn render(&self) -> Result<String> {
        let lock = Self {
            version: self.lowest_version(),
            ..self.clone()
        };
        let mut text = serde_json::to_string_pretty(&lock)?;
        text.push('\n');
        Ok(text)
    }

    /// Whether the lock records nothing, so it need not exist.
    pub fn is_empty(&self) -> bool {
        self.contracts.is_empty() && self.skills.is_empty()
    }

    fn lowest_version(&self) -> u32 {
        if self.skills.is_empty() && self.contracts.values().all(|entry| entry.files.is_empty()) {
            1
        } else {
            2
        }
    }

    fn check_origin(&self, path: &Path, source: &str, name: &str, commit: &str) -> Result<()> {
        ensure!(
            self.sources.contains_key(source),
            "{LOCK_FILE}: {} names unknown source {source}",
            path.display()
        );
        ensure!(
            is_kebab(name),
            "{LOCK_FILE}: {} has an invalid library name",
            path.display()
        );
        ensure!(
            commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
            "{LOCK_FILE}: {} has an invalid commit",
            path.display()
        );
        Ok(())
    }
}

/// Hash a `CONTRACT.md` for its entry's `hash`: its exact bytes without the `name` value, so a
/// locally renamed copy still matches upstream. This is the version 1 meaning, which every lock
/// version keeps, so a stored hash never changes meaning.
pub fn contract_hash(document: &str) -> Result<String> {
    Ok(format!(
        "blake3:{}",
        blake3::hash(with_name(document, "")?.as_bytes()).to_hex()
    ))
}

/// Hash a `SKILL.md` for its entry's `files`, as [`file_hash`] does, without its `name` value.
pub fn skill_hash(document: &str) -> Result<String> {
    Ok(file_hash(with_name(document, "")?.as_bytes()))
}

/// Hash a file for an entry's `files`: with CRLF line endings read as LF, so Git's line-ending
/// settings never make an installed asset or skill file look edited.
pub fn file_hash(bytes: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    let mut rest = bytes;
    while let Some(index) = rest.windows(2).position(|pair| pair == b"\r\n") {
        hasher.update(&rest[..index]);
        rest = &rest[index + 1..];
    }
    hasher.update(rest);
    format!("blake3:{}", hasher.finalize().to_hex())
}

fn is_hash(value: &str) -> bool {
    value
        .strip_prefix("blake3:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Whether `path`, relative to a contract's folder, is a file a contract may ship besides its
/// `CONTRACT.md`: one under `assets/<tool>/`.
pub fn is_contract_asset(path: &Path) -> bool {
    let mut parts = path.components();
    is_relative(path)
        && parts
            .next()
            .is_some_and(|part| part.as_os_str() == "assets")
        && parts.count() >= 2
}

fn is_relative(path: &Path) -> bool {
    path.components().next().is_some()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn is_contract_path(path: &Path) -> bool {
    let parts: Vec<_> = path.components().collect();
    is_relative(path)
        && parts.len() >= 3
        && parts[parts.len() - 1].as_os_str() == "CONTRACT.md"
        && parts[parts.len() - 3].as_os_str() == ".contracts"
}
