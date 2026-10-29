//! Files in the review state directory, `.tenet/` by default: append-only JSON Lines and
//! content-addressed blobs.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read as _, Seek as _, SeekFrom, Write as _},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::git::{self, GitError};

/// The review state directory's name in the project root, unless `--state-dir` names another.
pub const DIRECTORY: &str = ".tenet";
pub(crate) const EVENTS: &str = "events.jsonl";
pub(crate) const ANSWERS: &str = "answers.jsonl";
/// The largest text `blobs/` keeps. Blobs only let `show` diff later edits, and larger files are
/// mostly generated, so keeping them would let a few files dominate the directory.
pub const MAX_BLOB_BYTES: usize = 1024 * 1024;

/// Why a file in the review state directory cannot be read or written.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("cannot append to {:?}", .path.display())]
    Append { path: PathBuf, source: io::Error },
    #[error("cannot create {:?}", .path.display())]
    Create { path: PathBuf, source: io::Error },
    #[error(transparent)]
    Encode(#[from] serde_json::Error),
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("{:?}:{number:?}: invalid JSON", .path.display())]
    InvalidJson {
        path: PathBuf,
        number: usize,
        source: serde_json::Error,
    },
    #[error("{:?}:{number:?}", .path.display())]
    InvalidLine {
        path: PathBuf,
        number: usize,
        source: serde_json::Error,
    },
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("cannot lock {:?}", .path.display())]
    Lock { path: PathBuf, source: io::Error },
    #[error("{:?}:{number:?}: the line has no format version", .path.display())]
    NoFormat { path: PathBuf, number: usize },
    #[error("cannot open {:?}", .path.display())]
    Open { path: PathBuf, source: io::Error },
    #[error("cannot write {:?}", .path.display())]
    Persist {
        path: PathBuf,
        source: tempfile::PersistError,
    },
    #[error("cannot read {:?}", .path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("cannot rewrite {:?}", .path.display())]
    Rewrite { path: PathBuf, source: io::Error },
    #[error(
        "Git tracks {directory:?}/{name:?}, but review state belongs to one machine: committed \
         records and cached answers would settle contracts in every clone. Untrack it with \
         `git rm -r --cached {directory:?}` and commit the removal",
        directory = .directory.display()
    )]
    Tracked { directory: PathBuf, name: String },
    #[error(
        "{:?} is shorter than when Tenet read it; it must only ever be appended to",
        .0.display()
    )]
    Truncated(PathBuf),
    #[error("{:?}:{number:?}: unsupported format {format:?}", .path.display())]
    UnsupportedFormat {
        path: PathBuf,
        number: usize,
        format: u64,
    },
    #[error("cannot write {:?}", .path.display())]
    Write { path: PathBuf, source: io::Error },
}

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

/// The complete lines after byte `start`, and the offset where they end.
///
/// A final line without a newline is a write still in progress, or one that never finished; it was
/// never acknowledged, so it is not part of the file.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "The offset plus the read length is bounded by the store file's size."
)]
#[expect(
    clippy::as_conversions,
    reason = "A usize fits in a u64 on every supported target."
)]
pub(crate) fn read(path: &Path, start: u64) -> Result<(String, u64), StoreError> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound && start == 0 => {
            return Ok((String::new(), 0));
        },
        Err(source) => {
            return Err(StoreError::Read {
                path: path.to_owned(),
                source,
            });
        },
    };
    let text = read_after(&mut file, path, start)?;
    let end = start + text.len() as u64;
    Ok((text, end))
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "`rfind` returns an index inside the text, so one past it is at most its length."
)]
#[expect(
    clippy::verbose_file_reads,
    reason = "The read continues from the seek offset on the locked file handle."
)]
fn read_after(file: &mut File, path: &Path, start: u64) -> Result<String, StoreError> {
    if file.metadata()?.len() < start {
        return Err(StoreError::Truncated(path.to_owned()));
    }
    let _: u64 = file.seek(SeekFrom::Start(start))?;
    let mut text = String::new();
    let _: usize = file.read_to_string(&mut text).map_err(|source| {
        StoreError::Read {
            path: path.to_owned(),
            source,
        }
    })?;
    text.truncate(text.rfind('\n').map_or(0, |end| end + 1));
    Ok(text)
}

/// Each nonblank line of `text` with its 1-based number, counting from `first`, and the format
/// version in its `v` field.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Line numbers are bounded by the store file's line count."
)]
pub(crate) fn read_lines<'text>(
    path: &'text Path,
    text: &'text str,
    first: usize,
) -> impl Iterator<Item = Result<(usize, u64, &'text str), StoreError>> + 'text {
    #[derive(Deserialize)]
    struct Format {
        #[serde(rename = "v")]
        version: Option<u64>,
    }

    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.is_empty())
        .map(move |(index, line)| {
            let number = first + index + 1;
            let format: Format = serde_json::from_str(line).map_err(|source| {
                StoreError::InvalidJson {
                    path: path.to_owned(),
                    number,
                    source,
                }
            })?;
            let version = format.version.ok_or_else(|| {
                StoreError::NoFormat {
                    path: path.to_owned(),
                    number,
                }
            })?;
            Ok((number, version, line))
        })
}

/// Append lines in `format` under an exclusive lock.
pub(crate) fn append<Line>(path: &Path, format: u64, lines: &[Line]) -> Result<(), StoreError>
where
    Line: Serialize,
{
    let mut file = lock(path)?;
    let _: usize = write(&mut file, path, format, lines)?;
    Ok(())
}

/// Append lines built from whatever other writers appended after byte `start`, under the lock, so
/// numbers derived from the log cannot collide with a concurrent writer's.
///
/// Returns the offset after the appended lines.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "The offset plus the appended length is bounded by the store file's size."
)]
#[expect(
    clippy::as_conversions,
    reason = "A usize fits in a u64 on every supported target."
)]
pub(crate) fn append_after<Line, Failure>(
    path: &Path,
    start: u64,
    format: u64,
    build: impl FnOnce(&str) -> Result<Vec<Line>, Failure>,
) -> Result<u64, Failure>
where
    Line: Serialize,
    Failure: From<StoreError>,
{
    let mut file = lock(path)?;
    let appended = read_after(&mut file, path, start)?;
    let lines = build(&appended)?;
    let written = write(&mut file, path, format, &lines)?;
    Ok(start + (appended.len() + written) as u64)
}

/// Replace the file's lines with the text `retain` returns for them, under the lock.
///
/// Only the
/// disposable answer cache shrinks. It is rewritten in place, not renamed over, so a check waiting
/// for the lock appends its answers after the kept lines instead of to a removed file.
pub(crate) fn replace_lines(
    path: &Path,
    retain: impl FnOnce(&str) -> Result<String, StoreError>,
) -> Result<(), StoreError> {
    let mut file = lock(path)?;
    let text = read_after(&mut file, path, 0)?;
    let kept = retain(&text)?;
    let failed = |source| {
        StoreError::Rewrite {
            path: path.to_owned(),
            source,
        }
    };
    file.set_len(0).map_err(failed)?;
    file.write_all(kept.as_bytes()).map_err(failed)?;
    file.sync_data()?;
    Ok(())
}

/// Open the file for appending and take its lock.
///
/// Only a writer holding the lock appends, so an
/// unterminated tail under the lock is a write that was interrupted. Remove it, or the next line
/// would join it.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "The subtraction runs only when the file length is above 0."
)]
#[expect(
    clippy::as_conversions,
    reason = "A usize fits in a u64 on every supported target."
)]
#[expect(
    clippy::verbose_file_reads,
    reason = "The read continues from the seek offset on the locked file handle."
)]
fn lock(path: &Path) -> Result<File, StoreError> {
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)
        .map_err(|source| {
            StoreError::Open {
                path: path.to_owned(),
                source,
            }
        })?;
    file.lock().map_err(|source| {
        StoreError::Lock {
            path: path.to_owned(),
            source,
        }
    })?;
    let length = file.metadata()?.len();
    if length > 0 {
        let mut last = [0];
        let _: u64 = file.seek(SeekFrom::Start(length - 1))?;
        file.read_exact(&mut last)?;
        if last[0] != b'\n' {
            let _: u64 = file.seek(SeekFrom::Start(0))?;
            let mut text = Vec::new();
            let _: usize = file.read_to_end(&mut text)?;
            let complete = text
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(0, |end| end + 1);
            file.set_len(complete as u64)?;
        }
    }
    Ok(file)
}

fn write<Line>(
    file: &mut File,
    path: &Path,
    format: u64,
    lines: &[Line],
) -> Result<usize, StoreError>
where
    Line: Serialize,
{
    #[expect(
        clippy::arbitrary_source_item_ordering,
        reason = "Field order is the serialized key order."
    )]
    #[derive(Serialize)]
    struct Versioned<'text, Line> {
        #[serde(rename = "v")]
        version: u64,
        #[serde(flatten)]
        line: &'text Line,
    }

    let mut bytes = Vec::new();
    for line in lines {
        serde_json::to_writer(
            &mut bytes,
            &Versioned {
                version: format,
                line,
            },
        )?;
        bytes.push(b'\n');
    }
    file.write_all(&bytes).map_err(|source| {
        StoreError::Append {
            path: path.to_owned(),
            source,
        }
    })?;
    file.sync_data()?;
    Ok(bytes.len())
}
