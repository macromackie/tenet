use alloc::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use super::{Entry, Lock, LockError, SkillEntry};
use crate::parse::is_kebab;

impl Lock {
    /// Refuse a contract entry with an invalid path, origin, or hash.
    pub(super) fn check_contract(&self, path: &Path, entry: &Entry) -> Result<(), LockError> {
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
        check_file_hashes(path, &entry.files)
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
    pub(super) fn check_skill(&self, folder: &Path, entry: &SkillEntry) -> Result<(), LockError> {
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
        check_file_hashes(folder, &entry.files)
    }
}

fn check_file_hashes(path: &Path, files: &BTreeMap<PathBuf, String>) -> Result<(), LockError> {
    if let Some(file) = files
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
