//! Files under `.tenet/`: append-only JSON Lines and content-addressed blobs.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde_json::Value;

pub(crate) const DIRECTORY: &str = ".tenet";
pub(crate) const EVENTS: &str = "events.jsonl";
pub(crate) const ANSWERS: &str = "answers.jsonl";
const FORMAT: u64 = 1;

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
        Ok(Self { directory })
    }

    pub(crate) fn existing(root: &Path) -> Option<Self> {
        let directory = root.join(DIRECTORY);
        directory.is_dir().then_some(Self { directory })
    }

    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }

    pub(crate) fn write_blob(&self, text: &str) -> Result<String> {
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        let path = self.directory.join("blobs").join(&hash);
        if !path.exists() {
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

/// Read every complete line. A final line without a newline is a write still in progress, or one
/// that never finished; it was never acknowledged, so it is not part of the log.
pub(crate) fn read(path: &Path) -> Result<Vec<Value>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("cannot read {}", path.display())),
    };
    let complete = text.rfind('\n').map_or("", |end| &text[..end]);
    complete
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.is_empty())
        .map(|(index, line)| {
            let mut value: Value = serde_json::from_str(line)
                .with_context(|| format!("{}:{}: invalid JSON", path.display(), index + 1))?;
            let format = value.get("v").and_then(Value::as_u64);
            if format != Some(FORMAT) {
                bail!(
                    "{}:{}: unsupported format {format:?}; this Tenet reads format {FORMAT}",
                    path.display(),
                    index + 1
                );
            }
            if let Value::Object(fields) = &mut value {
                fields.remove("v");
            }
            Ok(value)
        })
        .collect()
}

/// Append lines under an exclusive lock.
pub(crate) fn append(path: &Path, values: Vec<Value>) -> Result<()> {
    append_with(path, false, |_| Ok(values))
}

/// Append lines derived from the log as it stands while the lock is held, so numbers derived from
/// existing lines cannot collide with a concurrent writer's.
pub(crate) fn append_after(
    path: &Path,
    build: impl FnOnce(&[Value]) -> Result<Vec<Value>>,
) -> Result<()> {
    append_with(path, true, build)
}

fn append_with(
    path: &Path,
    read_existing: bool,
    build: impl FnOnce(&[Value]) -> Result<Vec<Value>>,
) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)
        .with_context(|| format!("cannot open {}", path.display()))?;
    file.lock()
        .with_context(|| format!("cannot lock {}", path.display()))?;
    discard_unfinished_line(&mut file)?;
    let existing = if read_existing {
        read(path)?
    } else {
        Vec::new()
    };
    let mut bytes = Vec::new();
    for mut value in build(&existing)? {
        if let Value::Object(fields) = &mut value {
            fields.insert("v".into(), FORMAT.into());
        }
        serde_json::to_writer(&mut bytes, &value)?;
        bytes.push(b'\n');
    }
    file.write_all(&bytes)
        .with_context(|| format!("cannot append to {}", path.display()))?;
    file.sync_data()?;
    Ok(())
}

/// Only a writer holding the lock appends, so an unterminated tail under the lock is a write that
/// was interrupted. Remove it before appending, or the next line would join it.
fn discard_unfinished_line(file: &mut File) -> Result<()> {
    let length = file.metadata()?.len();
    if length == 0 {
        return Ok(());
    }
    let mut last = [0];
    file.seek(SeekFrom::Start(length - 1))?;
    file.read_exact(&mut last)?;
    if last[0] == b'\n' {
        return Ok(());
    }
    file.seek(SeekFrom::Start(0))?;
    let mut text = Vec::new();
    file.read_to_end(&mut text)?;
    let complete = text
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |end| end + 1);
    file.set_len(complete as u64)?;
    Ok(())
}
