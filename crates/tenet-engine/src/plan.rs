mod context;
mod excerpt;
mod selection;
mod span;

use std::{io, path::PathBuf};

pub(crate) use context::is_source;
pub use span::{SpanError, SpanRequest};

use crate::{
    ContextProfile,
    Plan,
    change::{ExcerptError, SourceError},
};

impl Plan {
    /// The largest encoded file or combined assessment input under this plan's profile.
    #[must_use]
    #[inline]
    pub const fn read_input_limit_bytes(&self) -> usize {
        self.context_profile.read_max_evidence_bytes()
    }

    /// Select the provider context profile before adding explicit context or capturing evidence.
    #[must_use]
    #[inline]
    pub const fn with_context_profile(mut self, profile: ContextProfile) -> Self {
        self.context_profile = profile;
        self
    }
}

/// Why `--context` cannot be supplied or sent.
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error(transparent)]
    Encode(#[from] serde_json::Error),
    #[error(transparent)]
    Excerpt(#[from] ExcerptError),
    #[error(transparent)]
    Inspect(io::Error),
    #[error("context file does not exist: {:?}", .0.display())]
    Missing(PathBuf),
    #[error("context paths must be repository source files: {:?}", .0.display())]
    NotSource(PathBuf),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(
        "supplementary context is {bytes:?} bytes as the model receives it; limit is \
         {limit_bytes:?} bytes; supply only the lines needed with --context PATH:START-END"
    )]
    TooLarge { bytes: usize, limit_bytes: usize },
}
