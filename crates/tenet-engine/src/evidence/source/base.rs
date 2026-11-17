use alloc::sync::Arc;
use std::{
    io,
    path::{Path, PathBuf},
    process::Stdio,
};

use tokio::process::Command;

use super::{super::CaptureError, Side, reader::read_stream};
use crate::{GitError, SourceError, Unreadable, git};

struct Blob {
    bytes: u64,
    is_symbolic: bool,
    object: String,
    root: PathBuf,
}

fn resolve_blob(root: &Path, base: &str, path: &Path) -> Result<Option<Blob>, SourceError> {
    let top = git::find_top(root)?;
    let full = root.join(path);
    let relative = full
        .strip_prefix(&top)?
        .to_str()
        .ok_or(SourceError::NotUtf8Path)?;
    let listing = git::run_git(
        &top,
        &["--literal-pathspecs", "ls-tree", "-z", base, "--", relative],
    )?;
    if listing.is_empty() {
        return Ok(None);
    }
    let is_symbolic = listing.starts_with(b"120000 ");
    if !(is_symbolic || listing.starts_with(b"100644 ") || listing.starts_with(b"100755 ")) {
        return Err(SourceError::BaseNotRegularFile(relative.to_owned()));
    }
    let object = format!("{base}:{relative}");
    let bytes = String::from_utf8(git::run_git(&top, &["cat-file", "-s", &object])?)?
        .trim()
        .parse()?;
    Ok(Some(Blob {
        bytes,
        is_symbolic,
        object,
        root: top,
    }))
}

async fn capture(
    root: &Path,
    base: &str,
    path: &Path,
    retain_bytes: usize,
) -> Result<Side, SourceError> {
    let Some(blob) = resolve_blob(root, base, path)? else {
        return Ok(Side::Absent);
    };
    let mut child = Command::new("git")
        .arg("-C")
        .arg(&blob.root)
        .args(["cat-file", "blob", &blob.object])
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(GitError::Run)?;
    let mut output = child.stdout.take().ok_or_else(|| {
        SourceError::Read {
            path: path.to_owned(),
            source: io::Error::other("Git blob reader has no output"),
        }
    })?;
    let captured = read_stream(path, &mut output, blob.bytes, retain_bytes).await;
    if captured.is_err() {
        child.kill().await.map_err(GitError::Run)?;
    }
    let status = child.wait().await.map_err(GitError::Run)?;
    if !status.success() {
        return Err(GitError::Failed {
            command: "cat-file".into(),
            stderr: status.to_string(),
        }
        .into());
    }
    let mut source = captured?;
    if blob.is_symbolic {
        source.text = Err(Arc::new(CaptureError::Source(
            Unreadable::SymbolicLink.into(),
        )));
    }
    Ok(Side::Captured(source))
}

pub(super) async fn read_base(root: &Path, base: &str, path: &Path, retain_bytes: usize) -> Side {
    match capture(root, base, path, retain_bytes).await {
        Ok(side) => side,
        Err(error) => Side::Unavailable(Arc::new(CaptureError::Source(error))),
    }
}
