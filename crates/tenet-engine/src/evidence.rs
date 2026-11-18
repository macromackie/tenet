mod capture;
mod context;
mod export;
mod file;
mod hunks;
mod identity;
mod record;
mod scope;
mod source;

use alloc::{collections::BTreeMap, sync::Arc};
use std::{io, path::PathBuf};

pub use hunks::HunkInput;
pub use record::{CapturedFile, Role, ScopedFile};
pub(crate) use scope::Scope;
pub(crate) use source::{is_stable_metadata, read_record_source};

use crate::{
    Plan,
    change::{ExcerptError, InputLimit, SourceError, Unreadable},
    report::Causes,
};

/// Why evidence cannot be captured or exported within its limits.
#[derive(Debug, thiserror::Error)]
pub enum EvidenceError {
    #[error(
        "captured inputs exceed {0:?} encoded bytes; select fewer paths or contracts; input was \
         not truncated"
    )]
    CaptureTooLarge(usize),
    #[error(transparent)]
    Context(#[from] crate::ContextError),
    #[error("contract documents exceed evidence limit of {0:?} bytes; input was not truncated")]
    ContractsTooLarge(usize),
    #[error(transparent)]
    Encode(#[from] serde_json::Error),
    #[error("evidence limit must be between 1 and 67108864 bytes")]
    InvalidLimit,
    #[error("captured source exceeds evidence limit of {0:?} bytes; input was not truncated")]
    SourceTooLarge(usize),
    #[error(transparent)]
    Write(#[from] io::Error),
}

/// Why a selected or supporting file's change was not captured. Every check of the file reports
/// it.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error(transparent)]
    Excerpt(#[from] ExcerptError),
    #[error(transparent)]
    Limit(#[from] InputLimit),
    #[error(transparent)]
    Source(#[from] SourceError),
}

impl CaptureError {
    pub(crate) const fn is_size_limit(&self) -> bool {
        match self {
            Self::Limit(_) => true,
            Self::Source(source) => matches!(source, SourceError::CaptureLimit { .. }),
            Self::Excerpt(_) => false,
        }
    }
}

/// Why a file's captured change cannot be judged, naming the file.
#[derive(Clone, Debug, thiserror::Error)]
pub enum ChangeError {
    #[error("source was not captured: {:?}", .0.display())]
    NotCaptured(PathBuf),
    #[error("{:?}: {:#}", .path.display(), Causes(&**.error))]
    Uncaptured {
        path: PathBuf,
        error: Arc<CaptureError>,
    },
    /// The file has no text a model can judge.
    #[error("{:?}: {reason:#}", .path.display())]
    Unreadable { path: PathBuf, reason: Unreadable },
}

/// Why selected or combined model input cannot be built.
#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error(transparent)]
    Change(#[from] ChangeError),
    #[error(transparent)]
    Encode(#[from] serde_json::Error),
    #[error(transparent)]
    Excerpt(#[from] ExcerptError),
    #[error(transparent)]
    Limit(#[from] InputLimit),
    #[error("inventory path is not UTF-8")]
    NotUtf8Path,
    #[error(transparent)]
    Unreadable(#[from] Unreadable),
}

impl InputError {
    pub(crate) fn is_size_limit(&self) -> bool {
        match self {
            Self::Limit(_) => true,
            Self::Change(ChangeError::Uncaptured { error, .. }) => error.is_size_limit(),
            Self::Change(ChangeError::NotCaptured(_) | ChangeError::Unreadable { .. })
            | Self::Encode(_)
            | Self::Excerpt(_)
            | Self::NotUtf8Path
            | Self::Unreadable(_) => false,
        }
    }
}

/// Source captured before assessment, shared by every request in one run.
#[derive(Debug)]
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "The sibling modules fill these fields, and the type stays private to its parent \
              module."
)]
pub struct EvidencePacket<'plan> {
    changes: BTreeMap<PathBuf, source::CapturedSource>,
    pub(crate) excerpts: BTreeMap<PathBuf, crate::Excerpt>,
    pub(crate) plan: &'plan Plan,
}

/// How a file whose model input cannot be built is reported: skipped when it has no text,
/// incomplete when its patch alone exceeds the input limit, and otherwise an error.
#[must_use]
#[inline]
pub fn classify_input_status(error: &InputError) -> (crate::Status, String) {
    match error {
        InputError::Unreadable(reason)
        | InputError::Change(ChangeError::Unreadable { reason, .. }) => {
            (crate::Status::Skipped, reason.to_string())
        },
        InputError::Limit(_) => (crate::Status::Incomplete, error.to_string()),
        InputError::Change(ChangeError::Uncaptured {
            error: captured, ..
        }) if captured.is_size_limit() => (crate::Status::Incomplete, error.to_string()),
        InputError::Change(ChangeError::Uncaptured { .. } | ChangeError::NotCaptured(_))
        | InputError::Excerpt(_)
        | InputError::Encode(_)
        | InputError::NotUtf8Path => (crate::Status::Error, Causes(error).to_string()),
    }
}

pub(crate) fn hash_content(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}
