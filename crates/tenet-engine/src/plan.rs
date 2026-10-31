mod context;
mod excerpt;
mod selection;
mod span;

use std::{io, path::PathBuf};

pub(crate) use context::is_source;
use ev_grep_core::MAX_FILE_BYTES;
pub use span::{SpanError, SpanRequest};

use crate::change::{ExcerptError, SourceError};

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
        "supplementary context is {0:?} bytes as the model receives it; limit is \
         {MAX_FILE_BYTES:?} bytes; supply only the lines needed with --context PATH:START-END"
    )]
    TooLarge(usize),
}
