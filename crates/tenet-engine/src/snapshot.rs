use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

use tempfile::TempDir;

use crate::{
    DiscoveryError,
    discovery,
    git::{self, GitError},
};

/// Why a snapshot directory and patch cannot become a checkout to review.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error(transparent)]
    Discovery(#[from] DiscoveryError),
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("invalid snapshot directory")]
    InvalidDirectory(#[source] io::Error),
    #[error("snapshot must be a directory")]
    NotDirectory,
    #[error("snapshot input must be a regular file: {:?}", .0.display())]
    NotRegularFile(PathBuf),
    #[error("patch path is not UTF-8")]
    NotUtf8PatchPath,
    #[error(transparent)]
    Prepare(io::Error),
    #[error("cannot read patch")]
    ReadPatch(#[source] io::Error),
}

/// A snapshot directory, with an optional patch applied, ready to check.
#[derive(Debug)]
#[expect(
    clippy::partial_pub_fields,
    reason = "The private fields hold state that only this crate may set."
)]
pub struct PreparedSnapshot {
    directory: TempDir,
    pub patch_hash: Option<String>,
    pub snapshot: PathBuf,
}

impl PreparedSnapshot {
    /// Copy a snapshot and apply `patch` to it, when one is given.
    #[inline]
    #[expect(
        clippy::too_many_lines,
        reason = "The steps run in one order and share local state; splitting them would add \
                  forwarding functions."
    )]
    pub fn new(snapshot: &Path, patch: Option<&Path>) -> Result<Self, SnapshotError> {
        let normalized = snapshot
            .canonicalize()
            .map_err(SnapshotError::InvalidDirectory)?;
        if !normalized.is_dir() {
            return Err(SnapshotError::NotDirectory);
        }
        let patch_bytes = patch
            .map(fs::read)
            .transpose()
            .map_err(SnapshotError::ReadPatch)?;
        let directory = tempfile::tempdir().map_err(SnapshotError::Prepare)?;
        copy(&normalized, directory.path())?;
        if !normalized.join(".contracts").exists()
            && let Some(parent) = normalized.parent()
            && parent.join(".contracts").is_dir()
        {
            copy(
                &parent.join(".contracts"),
                &directory.path().join(".contracts"),
            )?;
        }
        let root = directory.path();
        drop(git::run_git(root, &["init", "-q"])?);
        drop(git::run_git(root, &["add", "-A"])?);
        drop(git::run_git(
            root,
            &[
                "-c",
                "user.name=Tenet",
                "-c",
                "user.email=tenet@example.invalid",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-qm",
                "snapshot",
            ],
        )?);
        if let Some(bytes) = &patch_bytes {
            // Apply the bytes we hashed, even if the original patch changes during preparation.
            let captured = tempfile::NamedTempFile::new().map_err(SnapshotError::Prepare)?;
            fs::write(captured.path(), bytes).map_err(SnapshotError::Prepare)?;
            drop(git::run_git(
                root,
                &[
                    "apply",
                    "--index",
                    "--",
                    captured
                        .path()
                        .to_str()
                        .ok_or(SnapshotError::NotUtf8PatchPath)?,
                ],
            )?);
        }
        Ok(Self {
            directory,
            patch_hash: patch_bytes.map(|bytes| blake3::hash(&bytes).to_hex().to_string()),
            snapshot: normalized,
        })
    }

    /// Return the directory that holds the prepared snapshot.
    #[must_use]
    #[inline]
    pub fn read_root(&self) -> &Path {
        self.directory.path()
    }
}

fn copy(source: &Path, destination: &Path) -> Result<(), SnapshotError> {
    fs::create_dir_all(destination).map_err(SnapshotError::Prepare)?;
    for path in discovery::list_paths(source)? {
        let from = source.join(&path);
        if !from
            .symlink_metadata()
            .map_err(SnapshotError::Prepare)?
            .is_file()
        {
            return Err(SnapshotError::NotRegularFile(from));
        }
        let to = destination.join(path);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(SnapshotError::Prepare)?;
        }
        let _: u64 = fs::copy(from, to).map_err(SnapshotError::Prepare)?;
    }
    Ok(())
}
