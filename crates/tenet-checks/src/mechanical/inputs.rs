use alloc::collections::BTreeMap;
use std::{
    fs::{self, File},
    io::{self, Read as _},
    path::{Path, PathBuf},
};

use globset::GlobBuilder;
use sha2::{Digest as _, Sha256};
use tenet_engine::{MAX_CAPTURE_BYTES, is_stable_metadata};

use super::Freshness;

pub(super) struct Inputs<'tree> {
    bytes_left: usize,
    hashes: BTreeMap<PathBuf, Result<String, String>>,
    inventory: &'tree [PathBuf],
    root: &'tree Path,
}

impl<'tree> Inputs<'tree> {
    #[expect(
        clippy::unnecessary_debug_formatting,
        reason = "Quote dependency paths and escape control characters before printing them."
    )]
    pub(super) fn assess(
        &mut self,
        files: &BTreeMap<PathBuf, String>,
        discover: &[String],
    ) -> (Freshness, Option<String>) {
        for glob in discover {
            let matcher = match GlobBuilder::new(glob).literal_separator(true).build() {
                Ok(pattern) => pattern.compile_matcher(),
                Err(error) => return (Freshness::Unresolved, Some(error.to_string())),
            };
            let expected = files.keys().filter(|path| matcher.is_match(path));
            let current = self.inventory.iter().filter(|path| matcher.is_match(path));
            if !current.eq(expected) {
                return (
                    Freshness::Stale,
                    Some(format!("discovery membership changed for {glob:?}")),
                );
            }
        }
        for (path, expected) in files {
            if self.inventory.binary_search(path).is_err() {
                return (
                    Freshness::Stale,
                    Some(format!(
                        "dependency {:?} is absent from discovery",
                        path.as_os_str()
                    )),
                );
            }
            let observed = self
                .hashes
                .entry(path.clone())
                .or_insert_with(|| hash_file(self.root, path, &mut self.bytes_left));
            match observed {
                Ok(actual) if actual == expected => {},
                Ok(_) => {
                    return (
                        Freshness::Stale,
                        Some(format!("dependency {:?} changed", path.as_os_str())),
                    );
                },
                Err(error) => return (Freshness::Unresolved, Some(error.clone())),
            }
        }
        (Freshness::Current, None)
    }

    pub(super) const fn new(root: &'tree Path, inventory: &'tree [PathBuf]) -> Self {
        Self {
            bytes_left: MAX_CAPTURE_BYTES,
            hashes: BTreeMap::new(),
            inventory,
            root,
        }
    }
}

#[expect(
    clippy::unnecessary_debug_formatting,
    reason = "Quote dependency paths and escape control characters before printing them."
)]
fn hash_file(root: &Path, path: &Path, bytes_left: &mut usize) -> Result<String, String> {
    let file = root.join(path);
    let name = path.as_os_str();
    read_hash(&file, bytes_left)
        .map_err(|error| format!("cannot hash dependency {name:?}: {error:#}"))
}

pub(super) fn read_hash(path: &Path, bytes_left: &mut usize) -> io::Result<String> {
    let before = fs::symlink_metadata(path)?;
    if !before.is_file() || path.canonicalize()? != path {
        return Err(io::Error::other("expected a regular file without symlinks"));
    }
    let limit = u64::try_from(*bytes_left).unwrap_or(u64::MAX);
    let expected_bytes = before.len();
    if expected_bytes > limit {
        return Err(io::Error::other("dependency exceeds the capture limit"));
    }
    let file = File::open(path)?;
    if !is_stable_metadata(&before, &file.metadata()?) {
        return Err(io::Error::other("dependency changed before hashing"));
    }
    let mut digest = Sha256::new();
    let read_bytes = io::copy(
        &mut (&file).take(expected_bytes.saturating_add(1)),
        &mut digest,
    )?;
    *bytes_left = bytes_left.saturating_sub(usize::try_from(read_bytes).unwrap_or(usize::MAX));
    if read_bytes != expected_bytes
        || !is_stable_metadata(&before, &file.metadata()?)
        || !is_stable_metadata(&before, &fs::symlink_metadata(path)?)
        || path.canonicalize()? != path
    {
        return Err(io::Error::other("dependency changed during hashing"));
    }
    Ok(format!("{:x}", digest.finalize()))
}
