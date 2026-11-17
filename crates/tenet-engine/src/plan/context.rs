use std::{
    io,
    path::{Component, Path},
};

use crate::discovery::{is_allowed, is_contract};

/// Whether a caller may supply this path as repository source.
///
/// A path given as `--context` or as a record's evidence must be relative, outside excluded and
/// contract folders, and must not lead out of the canonical `root` through a symbolic link.
pub(crate) fn is_source(root: &Path, path: &Path) -> io::Result<bool> {
    let relative = path
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
        && is_allowed(path)
        && !is_contract(path);
    let full = root.join(path);
    Ok(relative && (!full.exists() || full.canonicalize()?.starts_with(root)))
}
