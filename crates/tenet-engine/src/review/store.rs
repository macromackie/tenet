//! Files under `.tenet/`: append-only JSON Lines and content-addressed blobs.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::git;

pub(crate) const DIRECTORY: &str = ".tenet";
pub(crate) const EVENTS: &str = "events.jsonl";
pub(crate) const ANSWERS: &str = "answers.jsonl";
/// The largest text `blobs/` keeps. Blobs only let `show` diff later edits, and larger files are
/// mostly generated, so keeping them would let a few files dominate the directory.
pub const MAX_BLOB_BYTES: usize = 1024 * 1024;

pub(crate) struct Store {
    directory: PathBuf,
}

impl Store {
    /// Create `.tenet/` with a `.gitignore` that keeps it out of Git and Tenet's own discovery.
    pub(crate) fn create(root: &Path) -> Result<Self> {
        let directory = root.join(DIRECTORY);
        fs::create_dir_all(directory.join("blobs"))
            .with_context(|| format!("cannot create {}", directory.display()))?;
        let ignore = directory.join(".gitignore");
        if !ignore.exists() {
            fs::write(&ignore, "*\n")
                .with_context(|| format!("cannot write {}", ignore.display()))?;
        }
        untracked(&directory)?;
        Ok(Self { directory })
    }

    pub(crate) fn existing(root: &Path) -> Result<Option<Self>> {
        let directory = root.join(DIRECTORY);
        if !directory.is_dir() {
            return Ok(None);
        }
        untracked(&directory)?;
        Ok(Some(Self { directory }))
    }

    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }

    /// Keep `text` under its hash, unless it exceeds `MAX_BLOB_BYTES`, and return the hash.
    pub(crate) fn write_blob(&self, text: &str) -> Result<String> {
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        let path = self.directory.join("blobs").join(&hash);
        if text.len() <= MAX_BLOB_BYTES && !path.exists() {
            let mut file = tempfile::NamedTempFile::new_in(self.directory.join("blobs"))?;
            file.write_all(text.as_bytes())?;
            file.persist(&path)
                .with_context(|| format!("cannot write {}", path.display()))?;
        }
        Ok(hash)
    }

    /// Captured contents, or `None` once someone has deleted the disposable blob directory.
    pub(crate) fn blob(&self, hash: &str) -> Result<Option<String>> {
        let path = self.directory.join("blobs").join(hash);
        match fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
        }
    }
}

/// Refuse review state that Git tracks. Records and cached answers describe one machine's review;
/// committed, they would settle contracts in every clone and in CI.
fn untracked(directory: &Path) -> Result<()> {
    // Outside a Git work tree nothing is tracked.
    if git::output(directory, &["rev-parse", "--is-inside-work-tree"]).is_err() {
        return Ok(());
    }
    let tracked = git::output(directory, &["ls-files", "-z", "--", "."])?;
    if let Some(name) = tracked
        .split(|byte| *byte == 0)
        .find(|name| !name.is_empty())
    {
        bail!(
            "Git tracks {DIRECTORY}/{}, but review state belongs to one machine: committed records \
             and cached answers would settle contracts in every clone. Untrack it with \
             `git rm -r --cached {DIRECTORY}` and commit the removal",
            String::from_utf8_lossy(name)
        );
    }
    Ok(())
}

/// The complete lines after byte `start`, and the offset where they end. A final line without a
/// newline is a write still in progress, or one that never finished; it was never acknowledged,
/// so it is not part of the file.
pub(crate) fn read(path: &Path, start: u64) -> Result<(String, u64)> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound && start == 0 => {
            return Ok((String::new(), 0));
        }
        Err(error) => return Err(error).with_context(|| format!("cannot read {}", path.display())),
    };
    let text = read_after(&mut file, path, start)?;
    let end = start + text.len() as u64;
    Ok((text, end))
}

fn read_after(file: &mut File, path: &Path, start: u64) -> Result<String> {
    ensure!(
        file.metadata()?.len() >= start,
        "{} is shorter than when Tenet read it; it must only ever be appended to",
        path.display()
    );
    file.seek(SeekFrom::Start(start))?;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .with_context(|| format!("cannot read {}", path.display()))?;
    text.truncate(text.rfind('\n').map_or(0, |end| end + 1));
    Ok(text)
}

/// Each nonblank line of `text` with its 1-based number, counting from `first`, and the format
/// version in its `v` field.
pub(crate) fn lines<'a>(
    path: &'a Path,
    text: &'a str,
    first: usize,
) -> impl Iterator<Item = Result<(usize, u64, &'a str)>> + 'a {
    #[derive(Deserialize)]
    struct Format {
        v: Option<u64>,
    }

    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.is_empty())
        .map(move |(index, line)| {
            let number = first + index + 1;
            let format: Format = serde_json::from_str(line)
                .with_context(|| format!("{}:{number}: invalid JSON", path.display()))?;
            let format = format.v.with_context(|| {
                format!(
                    "{}:{number}: the line has no format version",
                    path.display()
                )
            })?;
            Ok((number, format, line))
        })
}

/// Append lines in `format` under an exclusive lock.
pub(crate) fn append<T: Serialize>(path: &Path, format: u64, lines: &[T]) -> Result<()> {
    let mut file = lock(path)?;
    write(&mut file, path, format, lines)?;
    Ok(())
}

/// Append lines built from whatever other writers appended after byte `start`, under the lock, so
/// numbers derived from the log cannot collide with a concurrent writer's. Returns the offset after
/// the appended lines.
pub(crate) fn append_after<T: Serialize>(
    path: &Path,
    start: u64,
    format: u64,
    build: impl FnOnce(&str) -> Result<Vec<T>>,
) -> Result<u64> {
    let mut file = lock(path)?;
    let appended = read_after(&mut file, path, start)?;
    let lines = build(&appended)?;
    let written = write(&mut file, path, format, &lines)?;
    Ok(start + (appended.len() + written) as u64)
}

/// Open the file for appending and take its lock. Only a writer holding the lock appends, so an
/// unterminated tail under the lock is a write that was interrupted. Remove it, or the next line
/// would join it.
fn lock(path: &Path) -> Result<File> {
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)
        .with_context(|| format!("cannot open {}", path.display()))?;
    file.lock()
        .with_context(|| format!("cannot lock {}", path.display()))?;
    let length = file.metadata()?.len();
    if length > 0 {
        let mut last = [0];
        file.seek(SeekFrom::Start(length - 1))?;
        file.read_exact(&mut last)?;
        if last[0] != b'\n' {
            file.seek(SeekFrom::Start(0))?;
            let mut text = Vec::new();
            file.read_to_end(&mut text)?;
            let complete = text
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(0, |end| end + 1);
            file.set_len(complete as u64)?;
        }
    }
    Ok(file)
}

fn write<T: Serialize>(file: &mut File, path: &Path, format: u64, lines: &[T]) -> Result<usize> {
    #[derive(Serialize)]
    struct Versioned<'a, T> {
        v: u64,
        #[serde(flatten)]
        line: &'a T,
    }

    let mut bytes = Vec::new();
    for line in lines {
        serde_json::to_writer(&mut bytes, &Versioned { v: format, line })?;
        bytes.push(b'\n');
    }
    file.write_all(&bytes)
        .with_context(|| format!("cannot append to {}", path.display()))?;
    file.sync_data()?;
    Ok(bytes.len())
}
