//! Read the project's adoption lock without modifying it.

use std::{fs, io, path::Path};

use tenet_contracts::{LOCK_FILE, Lock, LockError};

/// Why `.tenet.lock` cannot be read.
#[derive(Debug, thiserror::Error)]
pub enum LockFileError {
    #[error(transparent)]
    Format(#[from] LockError),
    #[error("cannot read {LOCK_FILE:?}")]
    Read(#[source] io::Error),
}

/// Read and parse `.tenet.lock` from `root`, returning an empty lock when the file is absent.
#[inline]
pub fn read_lock(root: &Path) -> Result<Lock, LockFileError> {
    let path = root.join(LOCK_FILE);
    if !path.exists() {
        return Ok(Lock::default());
    }
    let text = fs::read_to_string(path).map_err(LockFileError::Read)?;
    Ok(Lock::parse(&text)?)
}
