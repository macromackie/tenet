//! Read and atomically replace the project's adoption lock.

use std::{
    fs,
    io::{self, Write as _},
    path::Path,
};

use tempfile::NamedTempFile;
use tenet_contracts::{LOCK_FILE, Lock, LockError};

/// Why `.tenet.lock` cannot be read.
#[derive(Debug, thiserror::Error)]
pub enum LockFileError {
    #[error(transparent)]
    Format(#[from] LockError),
    #[error("cannot read {LOCK_FILE:?}")]
    Read(#[source] io::Error),
}

/// Why an atomic lock replacement failed before it could be committed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum WriteLockError {
    #[error(transparent)]
    Format(#[from] serde_json::Error),
    #[error("cannot write {LOCK_FILE:?}")]
    Persist(#[source] tempfile::PersistError),
    #[error("cannot write {LOCK_FILE:?}")]
    Write(#[source] io::Error),
}

pub(crate) fn read_lock(root: &Path) -> Result<Lock, LockFileError> {
    let path = root.join(LOCK_FILE);
    if !path.exists() {
        return Ok(Lock::default());
    }
    let text = fs::read_to_string(path).map_err(LockFileError::Read)?;
    Ok(Lock::parse(&text)?)
}

/// Write the lock, or delete it once it records nothing. The lock is renamed into place, so an
/// interrupted write leaves the previous lock whole.
pub(crate) fn write_lock(root: &Path, lock: &Lock) -> Result<(), WriteLockError> {
    let path = root.join(LOCK_FILE);
    if lock.is_empty() {
        if path.exists() {
            fs::remove_file(path).map_err(WriteLockError::Write)?;
        }
        return Ok(());
    }
    let mut file = NamedTempFile::new_in(root).map_err(WriteLockError::Write)?;
    file.write_all(lock.render()?.as_bytes())
        .map_err(WriteLockError::Write)?;
    // A temporary file starts readable only by its owner; the lock stays readable by everyone.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o644))
            .map_err(WriteLockError::Write)?;
    }
    drop(file.persist(path).map_err(WriteLockError::Persist)?);
    Ok(())
}
