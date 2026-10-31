use std::{
    io,
    path::{PathBuf, StripPrefixError},
};

use tenet_contracts::{DocumentError, LockError};

use crate::{git::GitError, lock::LockFileError};

/// Why the project's contracts or a check's files cannot be discovered.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("contract too large: {:?}", .0.display())]
    ContractTooLarge(PathBuf),
    #[error(transparent)]
    Document(#[from] DocumentError),
    #[error("duplicate contract name: {0:?}")]
    DuplicateName(String),
    #[error("duplicate contract number in {:?}", .0.display())]
    DuplicateNumber(PathBuf),
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("invalid project root")]
    InvalidRoot(#[source] io::Error),
    #[error(transparent)]
    Lock(#[from] LockError),
    #[error(transparent)]
    LockFile(#[from] LockFileError),
    #[error(transparent)]
    Metadata(io::Error),
    #[error("no contracts found under {:?}", .0.display())]
    NoContracts(PathBuf),
    #[error("contract must be a regular file: {:?}", .0.display())]
    NotRegularContract(PathBuf),
    #[error(transparent)]
    OutsideRoot(#[from] StripPrefixError),
    #[error("cannot read {:?}", .path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("selected path escapes project")]
    SelectedEscapes,
    #[error("selected path does not exist: {:?}", .path.display())]
    SelectedMissing { path: PathBuf, source: io::Error },
    #[error("selected paths must be relative and stay within the project")]
    SelectedOutside,
    #[error("selected paths must not be symbolic links")]
    SelectedSymbolicLink,
    #[error("unknown contract: {0:?}")]
    UnknownContract(String),
    #[error("file discovery failed")]
    Walk(#[source] ignore::Error),
}

impl DiscoveryError {
    /// The refused contract document, when this error is one.
    #[must_use]
    #[inline]
    pub const fn as_document(&self) -> Option<&DocumentError> {
        let Self::Document(error) = self else {
            return None;
        };
        Some(error)
    }
}
