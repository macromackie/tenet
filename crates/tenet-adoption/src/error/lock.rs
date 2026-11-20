//! Lock persistence error conversion into adoption errors.

use super::AdoptionError;
use crate::lock::WriteLockError;

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
