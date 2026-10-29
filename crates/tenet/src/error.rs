//! Why a command failed.

use std::io;

use tenet_engine::{
    CheckError,
    ContextError,
    DiscoveryError,
    EvidenceError,
    SnapshotError,
    adoption::AdoptionError,
    review::{ReviewError, StoreError},
};

/// A command's failure.
///
/// `main` stays quiet when a write failed because a reader closed the
/// output early, and diagnostics annotate the document a discovery, adoption or review error
/// rejects. A failure no caller branches on stays `Other`.
#[derive(Debug, thiserror::Error)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Variants follow the order a command meets each failure."
)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Discovery(#[from] DiscoveryError),
    #[error(transparent)]
    Adoption(#[from] AdoptionError),
    #[error(transparent)]
    Review(#[from] ReviewError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
    #[error(transparent)]
    Context(#[from] ContextError),
    #[error(transparent)]
    Evidence(#[from] EvidenceError),
    /// The provider's model, endpoint, key, or job count is invalid.
    #[error(transparent)]
    Provider(#[from] ev_grep_jev::JevError),
    #[error(transparent)]
    Check(#[from] CheckError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Writing to standard output or standard error, or waiting for Ctrl-C, failed.
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
