//! Files in the review state directory, `.tenet/` by default: append-only JSON Lines and
//! content-addressed blobs.

mod error;
mod journal;

use std::{
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

pub use error::StoreError;
pub(super) use journal::{append, append_after, read, read_lines, replace_lines};

use crate::git;

/// The review state directory's name in the project root, unless `--state-dir` names another.
pub const DIRECTORY: &str = ".tenet";
pub(crate) const EVENTS: &str = "events.jsonl";
pub(crate) const ANSWERS: &str = "answers.jsonl";
/// The largest text `blobs/` keeps. Blobs only let `show` diff later edits, and larger files are
/// mostly generated, so keeping them would let a few files dominate the directory.
pub const MAX_BLOB_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub(crate) struct Store {
    directory: PathBuf,
}

impl Store {
    /// Create `directory` with a `.gitignore` that keeps it out of Git and Tenet's own discovery.
    pub(crate) fn create(directory: &Path) -> Result<Self, StoreError> {
        let owned = directory.to_path_buf();
        fs::create_dir_all(owned.join("blobs")).map_err(|source| {
            StoreError::Create {
                path: owned.clone(),
                source,
            }
        })?;
        let ignore = owned.join(".gitignore");
        if !ignore.exists() {
            fs::write(&ignore, "*\n").map_err(|source| {
                StoreError::Write {
                    path: ignore.clone(),
                    source,
                }
            })?;
        }
        ensure_untracked(&owned)?;
        Ok(Self { directory: owned })
    }

    pub(crate) fn open_existing(directory: &Path) -> Result<Option<Self>, StoreError> {
        let owned = directory.to_path_buf();
        if !owned.is_dir() {
            return Ok(None);
        }
        ensure_untracked(&owned)?;
        Ok(Some(Self { directory: owned }))
    }

    /// Captured contents, or `None` once someone has deleted the disposable blob directory.
    pub(crate) fn read_blob(&self, hash: &str) -> Result<Option<String>, StoreError> {
        let path = self.directory.join("blobs").join(hash);
        match fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(StoreError::Read { path, source }),
        }
    }

    pub(crate) fn resolve_path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }

    /// Keep `text` under its hash, unless it exceeds `MAX_BLOB_BYTES`, and return the hash.
    pub(crate) fn write_blob(&self, text: &str) -> Result<String, StoreError> {
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        let path = self.directory.join("blobs").join(&hash);
        if text.len() <= MAX_BLOB_BYTES && !path.exists() {
            let mut file = tempfile::NamedTempFile::new_in(self.directory.join("blobs"))?;
            file.write_all(text.as_bytes())?;
            drop(file.persist(&path).map_err(|source| {
                StoreError::Persist {
                    path: path.clone(),
                    source,
                }
            })?);
        }
        Ok(hash)
    }
}

/// Refuse review state that Git tracks. Records and cached answers describe one machine's review;
/// committed, they would settle contracts in every clone and in CI.
fn ensure_untracked(directory: &Path) -> Result<(), StoreError> {
    // Outside a Git work tree nothing is tracked.
    if git::run_git(directory, &["rev-parse", "--is-inside-work-tree"]).is_err() {
        return Ok(());
    }
    let tracked = git::run_git(directory, &["ls-files", "-z", "--", "."])?;
    if let Some(name) = tracked
        .split(|byte| *byte == 0)
        .find(|name| !name.is_empty())
    {
        return Err(StoreError::Tracked {
            directory: directory.to_owned(),
            name: String::from_utf8_lossy(name).into_owned(),
        });
    }
    Ok(())
}
