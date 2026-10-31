//! Failures of review-state files retain the failing path and original cause.

use std::{io, path::PathBuf};

use crate::git::GitError;

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
