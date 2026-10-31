//! File operation failures presented by adoption commands.

use core::str::Utf8Error;

use tenet_contracts::FrontmatterError;

use super::AdoptionError;
use crate::lock::WriteLockError;

/// Why a copy's file cannot be hashed as the lock records it.
#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum HashError {
    #[error(transparent)]
    Frontmatter(#[from] FrontmatterError),
    #[error(transparent)]
    NotUtf8(#[from] Utf8Error),
}

impl From<WriteLockError> for AdoptionError {
    #[inline]
    fn from(error: WriteLockError) -> Self {
        match error {
            WriteLockError::Format(source) => Self::RenderLock(source),
            WriteLockError::Persist(source) => Self::PersistLock(source),
            WriteLockError::Write(source) => Self::WriteLock(source),
        }
    }
}
