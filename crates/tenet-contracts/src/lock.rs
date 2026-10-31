mod hashing;
mod validation;

use alloc::collections::BTreeMap;
use std::path::PathBuf;

use globset::{GlobBuilder, GlobSet};
pub use hashing::{hash_contract, hash_file, hash_skill};
use serde::{Deserialize, Serialize};
pub use validation::is_contract_asset;

use crate::parse::{find_conflict_marker, is_kebab};

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
