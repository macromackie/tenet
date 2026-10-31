use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use ignore::WalkBuilder;

use super::DiscoveryError;
use crate::lock;

/// Names never discovered at any depth: Git internals, review state, the adoption lock, and
/// environment files that may hold credentials.
///
/// Nested repositories and worktrees, which carry their own `.git`, are separate projects, as are
/// the directories the lock's `projects` names. Ignore files exclude everything else.
/// The names a scan never enters or reads.
const PROTECTED_NAMES: [&str; 4] = [".git", ".tenet", ".tenet.lock", ".env"];

fn is_protected(name: &OsStr) -> bool {
    let text = name.to_string_lossy();
    PROTECTED_NAMES.contains(&text.as_ref()) || text.starts_with(".env.")
}

pub(crate) fn list_paths(root: &Path) -> Result<Vec<PathBuf>, DiscoveryError> {
    let projects = lock::read_lock(root)?.compile_separate_projects()?;
    let prefix = root.to_owned();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            entry.depth() == 0
                || !(is_protected(entry.file_name())
                    || entry.file_type().is_some_and(|kind| kind.is_dir())
                        && (entry.path().join(".git").symlink_metadata().is_ok()
                            || entry
                                .path()
                                .strip_prefix(&prefix)
                                .is_ok_and(|path| projects.is_match(path))))
        })
        .build();
    let mut paths = Vec::new();
    for entry in walker {
        let dir_entry = entry.map_err(DiscoveryError::Walk)?;
        if dir_entry.file_type().is_some_and(|kind| !kind.is_dir()) {
            paths.push(dir_entry.path().strip_prefix(root)?.to_owned());
        }
    }
    paths.sort();
    Ok(paths)
}

pub(crate) fn is_contract(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == ".contracts")
}

pub(crate) fn is_allowed(path: &Path) -> bool {
    !path.components().any(|part| is_protected(part.as_os_str()))
}
