//! A lock entry of either kind, seen as an installed folder of locked files.

mod comparison;
mod entries;
mod filesystem;
mod state;
mod update;

use alloc::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(super) use filesystem::{decode_text, find_obstruction, read, write};

use super::{
    AdoptionError,
    Change,
    File,
    FileComparison,
    Files,
    Kind,
    LocalState,
    Origin,
    Outcome,
    Revision,
    Rule,
    Upstream,
    merge,
};

/// A lock entry, contract or skill, seen as an installed folder of locked files.
pub(super) struct Unit {
    pub commit: String,
    /// Locked hashes keyed by path relative to the folder, the CONTRACT.md or SKILL.md included.
    pub files: BTreeMap<PathBuf, String>,
    /// Relative to the project root.
    pub folder: PathBuf,
    /// The lock key: a contract's CONTRACT.md path or a skill's folder.
    pub key: PathBuf,
    pub origin: Origin,
    pub reference: String,
    pub source: String,
}

/// An update decided for one copy before anything is written.
pub(super) struct Planned {
    /// The locked hashes at the new release.
    pub files: BTreeMap<PathBuf, String>,
    /// Each file's outcome, keyed by path relative to the folder.
    pub outcomes: BTreeMap<PathBuf, Outcome>,
    /// Contents to write, keyed by path relative to the project root.
    pub writes: Vec<(PathBuf, File)>,
}

impl Unit {
    /// Every locked file, relative to the project root.
    pub(super) fn list_locked_paths(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.files.keys().map(|path| self.folder.join(path))
    }

    /// The CONTRACT.md or SKILL.md, relative to the project root.
    pub(super) fn name_document(&self) -> PathBuf {
        self.folder.join(self.read_kind().name_document())
    }

    pub(super) const fn read_kind(&self) -> Kind {
        self.origin.read_kind()
    }

    /// The error for a library that has no copy of this entry at its locked commit.
    pub(super) fn report_missing_at_lock(&self) -> AdoptionError {
        AdoptionError::MissingAtLock {
            commit: self.commit.clone(),
            name: self.origin.name().to_owned(),
        }
    }

    /// Separate the CONTRACT.md or SKILL.md's value from the other files' values.
    pub(super) fn split<Value>(
        &self,
        mut values: BTreeMap<PathBuf, Value>,
    ) -> Result<(Value, BTreeMap<PathBuf, Value>), AdoptionError> {
        let document = values
            .remove(Path::new(self.read_kind().name_document()))
            .ok_or_else(|| {
                AdoptionError::NoDocument {
                    key: self.key.clone(),
                    document: self.read_kind().name_document(),
                }
            })?;
        Ok((document, values))
    }
}
