use alloc::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use globset::{GlobBuilder, GlobSet};
use serde::{Deserialize, Serialize};

use crate::parse::{FrontmatterError, find_conflict_marker, is_kebab, with_name};

/// The path segments that name the current or parent directory.
const DOT_SEGMENTS: [&str; 2] = [".", ".."];

/// File name of the committed adoption record at the project root.
pub const LOCK_FILE: &str = ".tenet.lock";
/// Alias of the public library, always written so a lock stands on its own.
pub const DEFAULT_SOURCE: &str = "tenet";
/// The contract library that `tenet contracts add` reads when no source is named.
pub const DEFAULT_SOURCE_URL: &str = "https://github.com/macromackie/tenet-contracts";

/// The newest lock format this Tenet reads. Version 2 adds locked `files` and `skills`, and
/// version 3 adds `projects`.
const NEWEST_VERSION: u32 = 3;

/// Why Tenet refuses a `.tenet.lock`.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("{LOCK_FILE:?}: {path:?} lists {file:?}, which is not a path under assets/<tool>/")]
    AssetPath { path: PathBuf, file: PathBuf },
    #[error("{LOCK_FILE:?}: {path:?} has an invalid commit")]
    Commit { path: PathBuf },
    #[error("{LOCK_FILE:?}:{line:?}: unresolved merge conflict marker")]
    ConflictMarker { line: usize },
    #[error("{LOCK_FILE:?}: {path:?} is not a relative .contracts/NNN-name/CONTRACT.md path")]
    ContractPath { path: PathBuf },
    #[error("{LOCK_FILE:?}: {path:?} has an invalid hash for {file:?}")]
    FileHash { path: PathBuf, file: PathBuf },
    #[error("{LOCK_FILE:?}: {path:?} has an invalid hash")]
    Hash { path: PathBuf },
    #[error("invalid {LOCK_FILE:?}")]
    Json(#[source] serde_json::Error),
    #[error("{LOCK_FILE:?}: {path:?} has an invalid library name")]
    LibraryName { path: PathBuf },
    #[error(
        "{LOCK_FILE:?} version {version:?} needs a Tenet newer than {:?}",
        env!("CARGO_PKG_VERSION")
    )]
    NewerVersion { version: u32 },
    #[error(
        "{LOCK_FILE:?} version {version:?} cannot hold its content; files and skills need version \
         2, and projects version 3"
    )]
    OlderVersion { version: u32 },
    #[error("{LOCK_FILE:?}: invalid projects pattern `{pattern:?}`")]
    ProjectGlob {
        pattern: String,
        source: globset::Error,
    },
    #[error("{LOCK_FILE:?}: projects pattern `{pattern:?}` must be relative to the project root")]
    ProjectOutside { pattern: String },
    #[error(transparent)]
    Projects(#[from] globset::Error),
    #[error("{LOCK_FILE:?}: {folder:?} does not list SKILL.md")]
    SkillDocument { folder: PathBuf },
    #[error("{LOCK_FILE:?}: {folder:?} lists {file:?}, which is not a relative path")]
    SkillFile { folder: PathBuf, file: PathBuf },
    #[error("{LOCK_FILE:?}: skill folder {folder:?} is not a relative path")]
    SkillFolder { folder: PathBuf },
    #[error("{LOCK_FILE:?}: invalid source alias {alias:?}")]
    SourceAlias { alias: String },
    #[error("{LOCK_FILE:?}: invalid URL for source {alias:?}")]
    SourceUrl { alias: String },
    #[error("{LOCK_FILE:?}: {path:?} names unknown source {alias:?}")]
    UnknownSource { path: PathBuf, alias: String },
    #[error("unsupported {LOCK_FILE:?} version {version:?}")]
    UnsupportedVersion { version: u32 },
}

/// Contracts and skills installed from shared sources.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    clippy::partial_pub_fields,
    reason = "the field order is the key order of every committed `.tenet.lock`, and only \
              `Lock::render` writes `version`"
)]
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
    /// Glob patterns, relative to the project root, of directories that are separate projects,
    /// such as test fixtures with contracts of their own. See [`Lock::compile_separate_projects`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<String>,
}

/// One installed contract and the upstream revision that is its next merge base.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the field order is the key order of every committed `.tenet.lock`"
)]
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
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the field order is the key order of every committed `.tenet.lock`"
)]
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
    #[inline]
    fn default() -> Self {
        Self {
            version: 1,
            sources: BTreeMap::from([(DEFAULT_SOURCE.into(), DEFAULT_SOURCE_URL.into())]),
            contracts: BTreeMap::new(),
            skills: BTreeMap::new(),
            projects: Vec::new(),
        }
    }
}

impl Lock {
    /// Refuse a contract entry with an invalid path, origin, or hash.
    fn check_contract(&self, path: &Path, entry: &Entry) -> Result<(), LockError> {
        if !is_contract_path(path) {
            return Err(LockError::ContractPath {
                path: path.to_owned(),
            });
        }
        self.check_origin(path, &entry.source, &entry.contract, &entry.commit)?;
        if !is_hash(&entry.hash) {
            return Err(LockError::Hash {
                path: path.to_owned(),
            });
        }
        if let Some(file) = entry.files.keys().find(|file| !is_contract_asset(file)) {
            return Err(LockError::AssetPath {
                path: path.to_owned(),
                file: file.clone(),
            });
        }
        if let Some(file) = entry
            .files
            .iter()
            .find_map(|(file, hash)| (!is_hash(hash)).then_some(file))
        {
            return Err(LockError::FileHash {
                path: path.to_owned(),
                file: file.clone(),
            });
        }
        Ok(())
    }

    fn check_origin(
        &self,
        path: &Path,
        source: &str,
        name: &str,
        commit: &str,
    ) -> Result<(), LockError> {
        if !self.sources.contains_key(source) {
            return Err(LockError::UnknownSource {
                path: path.to_owned(),
                alias: source.to_owned(),
            });
        }
        if !is_kebab(name) {
            return Err(LockError::LibraryName {
                path: path.to_owned(),
            });
        }
        if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(LockError::Commit {
                path: path.to_owned(),
            });
        }
        Ok(())
    }

    /// Refuse a skill entry with an invalid folder, origin, file list, or hash.
    fn check_skill(&self, folder: &Path, entry: &SkillEntry) -> Result<(), LockError> {
        if !is_relative(folder) {
            return Err(LockError::SkillFolder {
                folder: folder.to_owned(),
            });
        }
        self.check_origin(folder, &entry.source, &entry.skill, &entry.commit)?;
        if !entry.files.contains_key(Path::new("SKILL.md")) {
            return Err(LockError::SkillDocument {
                folder: folder.to_owned(),
            });
        }
        if let Some(file) = entry.files.keys().find(|file| !is_relative(file)) {
            return Err(LockError::SkillFile {
                folder: folder.to_owned(),
                file: file.clone(),
            });
        }
        if let Some(file) = entry
            .files
            .iter()
            .find_map(|(file, hash)| (!is_hash(hash)).then_some(file))
        {
            return Err(LockError::FileHash {
                path: folder.to_owned(),
                file: file.clone(),
            });
        }
        Ok(())
    }

    /// The directories `projects` names. Discovery skips each one, as it skips a nested Git
    /// checkout, so its contracts and files belong only to that project.
    #[inline]
    pub fn compile_separate_projects(&self) -> Result<GlobSet, LockError> {
        let globs = self
            .projects
            .iter()
            .map(|pattern| {
                if pattern.is_empty()
                    || pattern.starts_with('/')
                    || pattern.ends_with('/')
                    || pattern.split('/').any(|part| DOT_SEGMENTS.contains(&part))
                {
                    return Err(LockError::ProjectOutside {
                        pattern: pattern.clone(),
                    });
                }
                GlobBuilder::new(pattern)
                    .literal_separator(true)
                    .build()
                    .map_err(|source| {
                        LockError::ProjectGlob {
                            pattern: pattern.clone(),
                            source,
                        }
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(GlobSet::new(&globs)?)
    }

    fn find_lowest_version(&self) -> u32 {
        if !self.projects.is_empty() {
            3
        } else if self.skills.is_empty()
            && self.contracts.values().all(|entry| entry.files.is_empty())
        {
            1
        } else {
            2
        }
    }

    /// Whether the lock records nothing, so it need not exist.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.contracts.is_empty() && self.skills.is_empty() && self.projects.is_empty()
    }

    /// Parse and validate the text of a `.tenet.lock` file.
    #[inline]
    pub fn parse(text: &str) -> Result<Self, LockError> {
        if let Some(line) = find_conflict_marker(text) {
            return Err(LockError::ConflictMarker { line });
        }
        let Header { version } = serde_json::from_str(text).map_err(LockError::Json)?;
        if version > NEWEST_VERSION {
            return Err(LockError::NewerVersion { version });
        }
        if version < 1 {
            return Err(LockError::UnsupportedVersion { version });
        }
        let lock: Self = serde_json::from_str(text).map_err(LockError::Json)?;
        if lock.find_lowest_version() > version {
            return Err(LockError::OlderVersion { version });
        }
        drop(lock.compile_separate_projects()?);
        for (alias, url) in &lock.sources {
            if !is_kebab(alias) {
                return Err(LockError::SourceAlias {
                    alias: alias.clone(),
                });
            }
            if url.is_empty() || url.starts_with('-') {
                return Err(LockError::SourceUrl {
                    alias: alias.clone(),
                });
            }
        }
        for (path, entry) in &lock.contracts {
            lock.check_contract(path, entry)?;
        }
        for (folder, entry) in &lock.skills {
            lock.check_skill(folder, entry)?;
        }
        Ok(lock)
    }

    /// Sorted, pretty JSON with a trailing newline, so independent additions touch separate lines.
    #[inline]
    pub fn render(&self) -> Result<String, serde_json::Error> {
        let lock = Self {
            version: self.find_lowest_version(),
            ..self.clone()
        };
        let mut text = serde_json::to_string_pretty(&lock)?;
        text.push('\n');
        Ok(text)
    }
}

/// Hash a `CONTRACT.md` for its entry's `hash`: its exact bytes without the `name` value.
///
/// A locally renamed copy still matches upstream. This is the version 1 meaning, which every lock
/// version keeps, so a stored hash never changes meaning.
#[inline]
pub fn hash_contract(document: &str) -> Result<String, FrontmatterError> {
    Ok(format!(
        "blake3:{}",
        blake3::hash(with_name(document, "")?.as_bytes()).to_hex()
    ))
}

/// Hash a file for an entry's `files`: with CRLF line endings read as LF, so Git's line-ending
/// settings never make an installed asset or skill file look edited.
#[must_use]
#[inline]
pub fn hash_file(bytes: &[u8]) -> String {
    let mut empty = blake3::Hasher::new();
    let hasher = bytes
        .split_inclusive(|byte| *byte == b'\n')
        .fold(&mut empty, |partial, line| {
            match line.strip_suffix(b"\r\n") {
                Some(text) => partial.update(text).update(b"\n"),
                None => partial.update(line),
            }
        });
    format!("blake3:{}", hasher.finalize().to_hex())
}

/// Hash a `SKILL.md` for its entry's `files`, as [`hash_file`] does, without its `name` value.
#[inline]
pub fn hash_skill(document: &str) -> Result<String, FrontmatterError> {
    Ok(hash_file(with_name(document, "")?.as_bytes()))
}

fn is_hash(value: &str) -> bool {
    value
        .strip_prefix("blake3:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn is_relative(path: &Path) -> bool {
    path.components().next().is_some()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

/// Whether `path`, relative to a contract's folder, is a file a contract may ship besides its
/// `CONTRACT.md`: one under `assets/<tool>/`.
#[must_use]
#[inline]
pub fn is_contract_asset(path: &Path) -> bool {
    let mut parts = path.components();
    is_relative(path)
        && parts
            .next()
            .is_some_and(|part| part.as_os_str() == "assets")
        && parts.count() >= 2
}

fn is_contract_path(path: &Path) -> bool {
    let mut parts = path.components().rev();
    is_relative(path)
        && parts
            .next()
            .is_some_and(|part| part.as_os_str() == "CONTRACT.md")
        && parts.next().is_some()
        && parts
            .next()
            .is_some_and(|part| part.as_os_str() == ".contracts")
}
