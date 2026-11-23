#[cfg(test)]
mod tests;

use alloc::collections::BTreeSet;
use std::{
    io,
    path::{Component, Path, PathBuf},
};

use sha2::{Digest as _, Sha256};
use tempfile::NamedTempFile;

use super::{
    Change,
    FixError,
    staging::{prepare_atomic_write, read_source, replace_staged_file},
};
use crate::behavior::AuditFixPatch;

/// Apply verified replacement patches from `tenet fix --from-audit` atomically after validating
/// every relative path, file type, and pre-edit SHA-256 hash against the live checkout.
#[inline]
pub fn apply_audit_patches(
    root: &Path,
    patches: &[AuditFixPatch],
) -> Result<Vec<Change>, FixError> {
    if patches.is_empty() {
        return Ok(Vec::new());
    }
    let canonical_root = root.canonicalize()?;
    let mut seen_paths = BTreeSet::new();
    let mut pending = Vec::new();
    let mut changes = Vec::new();
    for patch in patches {
        if let Some((relative, original, ready, change)) =
            prepare_audit_patch(&canonical_root, patch, &mut seen_paths)?
        {
            pending.push((relative, original, ready));
            changes.push(change);
        }
    }
    for (index, (relative, original, ready)) in pending.into_iter().enumerate() {
        if read_source(&canonical_root, &relative)? != original {
            return Err(io::Error::other(format!(
                "{:?} changed during audit fix. {index:?} files already updated",
                relative.display()
            ))
            .into());
        }
        replace_staged_file(&canonical_root, &relative, ready, index)?;
    }
    Ok(changes)
}

fn prepare_audit_patch(
    canonical_root: &Path,
    patch: &AuditFixPatch,
    seen_paths: &mut BTreeSet<PathBuf>,
) -> io::Result<Option<(PathBuf, String, NamedTempFile, Change)>> {
    let relative = validate_patch_path(&patch.path)?;
    if !seen_paths.insert(relative.clone()) {
        return Err(io::Error::other(format!(
            "duplicate audit fix patch path: {:?}",
            relative.display()
        )));
    }
    let original = read_source(canonical_root, &relative)?;
    let current_hash = format!("{:x}", Sha256::digest(original.as_bytes()));
    if current_hash != patch.original_hash {
        return Err(io::Error::other(format!(
            "{:?} changed before applying audit fix",
            relative.display()
        )));
    }
    if patch.content == original {
        return Ok(None);
    }
    let (ready, change) =
        prepare_atomic_write(canonical_root, &relative, &original, &patch.content)?;
    Ok(Some((relative, original, ready, change)))
}

fn validate_patch_path(raw: &str) -> io::Result<PathBuf> {
    let candidate = Path::new(raw);
    if raw.is_empty() || candidate.is_absolute() {
        return Err(io::Error::other(format!(
            "audit fix path must stay relative to the project root: {raw:?}"
        )));
    }
    let mut clean = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(segment) => clean.push(segment),
            Component::CurDir => {},
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err(io::Error::other(format!(
                    "audit fix path cannot escape the project root: {raw:?}"
                )));
            },
        }
    }
    if clean.as_os_str().is_empty() || !tenet_engine::is_allowed(&clean) {
        return Err(io::Error::other(format!(
            "audit fix path is not an allowed project source: {raw:?}"
        )));
    }
    Ok(clean)
}
