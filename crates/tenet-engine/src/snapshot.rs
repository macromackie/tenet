use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use tempfile::TempDir;

use crate::{discovery, git};

pub struct PreparedSnapshot {
    directory: TempDir,
    pub snapshot: PathBuf,
    pub patch_hash: Option<String>,
}

impl PreparedSnapshot {
    pub fn new(snapshot: &Path, patch: Option<&Path>) -> Result<Self> {
        let snapshot = snapshot
            .canonicalize()
            .context("invalid snapshot directory")?;
        ensure!(snapshot.is_dir(), "snapshot must be a directory");
        let patch_bytes = patch
            .map(fs::read)
            .transpose()
            .context("cannot read patch")?;
        let directory = tempfile::tempdir()?;
        copy(&snapshot, directory.path())?;
        if !snapshot.join(".contracts").exists()
            && let Some(parent) = snapshot.parent()
            && parent.join(".contracts").is_dir()
        {
            copy(
                &parent.join(".contracts"),
                &directory.path().join(".contracts"),
            )?;
        }
        let root = directory.path();
        git::output(root, &["init", "-q"])?;
        git::output(root, &["add", "-A"])?;
        git::output(
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
        )?;
        if let Some(bytes) = &patch_bytes {
            // Apply the bytes we hashed, even if the original patch changes during preparation.
            let captured = tempfile::NamedTempFile::new()?;
            fs::write(captured.path(), bytes)?;
            git::output(
                root,
                &[
                    "apply",
                    "--index",
                    "--",
                    captured
                        .path()
                        .to_str()
                        .context("patch path is not UTF-8")?,
                ],
            )?;
        }
        Ok(Self {
            directory,
            snapshot,
            patch_hash: patch_bytes.map(|bytes| blake3::hash(&bytes).to_hex().to_string()),
        })
    }

    pub fn root(&self) -> &Path {
        self.directory.path()
    }
}

fn copy(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for path in discovery::paths(source)? {
        let from = source.join(&path);
        ensure!(
            from.symlink_metadata()?.is_file(),
            "snapshot input must be a regular file: {}",
            from.display()
        );
        let to = destination.join(path);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(from, to)?;
    }
    Ok(())
}
