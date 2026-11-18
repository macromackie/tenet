use tokio::sync::AcquireError;

use crate::{RequestFailure, review::StoreError};

/// Why a check stopped before concluding every contract. A failure of the caller's `emit`
/// callback ends the check as the caller's own error instead.
#[derive(Debug, thiserror::Error)]
pub enum CheckError<RequestError> {
    #[error(transparent)]
    Cache(#[from] StoreError),
    /// A combined assessment failed in a way every later request would.
    #[error("{reason:?}, while assessing {contract:?}")]
    CombinedRequest {
        failure: RequestFailure,
        reason: &'static str,
        contract: String,
        source: Box<RequestError>,
    },
    #[error(transparent)]
    Encode(#[from] serde_json::Error),
    /// A file's request failed in a way every later request would; `failure` names its kind.
    #[error("{reason:?}, while checking {path:?} against {contract:?}")]
    FileRequest {
        failure: RequestFailure,
        reason: &'static str,
        path: String,
        contract: String,
        source: Box<RequestError>,
    },
    #[error("a shared assessment returned an incomplete answer set")]
    InvalidBatchAnswers,
    #[error("contract-batch-size must be between 1 and 64")]
    InvalidBatchSize,
    #[error("min-confidence must be between 0 and 1")]
    InvalidConfidence,
    #[error("jobs must be between 1 and 256")]
    InvalidJobs,
    #[error("measuring the provider request failed: {0:#}")]
    Measure(RequestError),
    #[error(transparent)]
    Slots(#[from] AcquireError),
}
