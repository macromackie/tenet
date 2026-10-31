//! Locked JSON Lines reads and writes preserve completed entries and append offsets.

use std::{
    fs::{File, OpenOptions},
    io::{self, Read as _, Seek as _, SeekFrom, Write as _},
    path::Path,
};

use serde::{Deserialize, Serialize};

use super::StoreError;

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
pub(in crate::review) fn read(path: &Path, start: u64) -> Result<(String, u64), StoreError> {
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
pub(in crate::review) fn read_lines<'text>(
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
pub(in crate::review) fn append<Line>(
    path: &Path,
    format: u64,
    lines: &[Line],
) -> Result<(), StoreError>
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
pub(in crate::review) fn append_after<Line, Failure>(
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
pub(in crate::review) fn replace_lines(
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
