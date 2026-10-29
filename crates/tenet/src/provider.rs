//! The model a check sends requests to, and what kind of failure a Jev request met.

use ev_grep_core::{ErrorKind, Failure};
use ev_grep_jev::{FailureKind as JevFailure, Jev, RequestError};
use tenet_engine::{
    Assessment,
    Candidate,
    ClassifyRequest,
    DecisionBatch,
    Evaluator,
    FailureKind,
    Question,
    RequestFailure,
};

/// The provider's evaluator, or none for a check whose contracts can send no request.
pub(crate) enum Model {
    Jev(Box<Jev>),
    Unneeded,
}

/// Why the model answered no request.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ModelError {
    #[error(transparent)]
    Core(#[from] ev_grep_core::InputError),
    #[error(transparent)]
    Jev(#[from] ev_grep_jev::JevError),
    #[error("this check has no contract that sends a model request")]
    Unneeded,
}

impl Failure for ModelError {
    fn classify(&self) -> ErrorKind {
        match self {
            Self::Core(error) => error.classify(),
            Self::Jev(error) => error.classify(),
            Self::Unneeded => ErrorKind::Configuration,
        }
    }
}

impl ClassifyRequest for ModelError {
    /// Name the failure from Jev's typed request error, with retryability from Jev's retry
    /// table. An error raised before any request is sent is never retryable.
    fn classify_request(&self) -> RequestFailure {
        match self {
            Self::Jev(ev_grep_jev::JevError::Request(error)) => classify_request(error),
            Self::Core(_) | Self::Jev(_) | Self::Unneeded => {
                let kind = if self.classify() == ErrorKind::TooLarge {
                    FailureKind::InputTooLarge
                } else {
                    FailureKind::Unknown
                };
                RequestFailure::new(kind, false)
            },
        }
    }
}

/// Map a Jev request failure to the engine's kind. Jev groups every other refusal as
/// `Rejected`, so its HTTP status separates a missing endpoint, an oversized input, and a closed
/// connection.
const fn classify_request(error: &RequestError) -> RequestFailure {
    let kind = match error.kind {
        JevFailure::Timeout => FailureKind::Timeout,
        JevFailure::Connect | JevFailure::Tls | JevFailure::Transport => FailureKind::Connection,
        JevFailure::RateLimited => FailureKind::RateLimited,
        JevFailure::Unavailable => FailureKind::ServerError,
        JevFailure::Moderation => FailureKind::Moderation,
        JevFailure::CredentialRejected => FailureKind::Authentication,
        JevFailure::ModelMismatch => FailureKind::Configuration,
        JevFailure::InvalidResponse | JevFailure::ResponseTooLarge => FailureKind::InvalidResponse,
        JevFailure::Rejected => {
            match error.status {
                Some(300..=399 | 404 | 405) => FailureKind::Configuration,
                Some(413) => FailureKind::InputTooLarge,
                Some(499) => FailureKind::Interrupted,
                _ => FailureKind::InvalidRequest,
            }
        },
    };
    RequestFailure::new(kind, error.kind.is_retryable())
}

impl Evaluator for Model {
    type Error = ModelError;

    async fn assess(&self, query: &str, candidate: &Candidate) -> Result<Assessment, ModelError> {
        match self {
            Self::Jev(jev) => Ok(jev.assess(query, candidate).await?),
            Self::Unneeded => Err(ModelError::Unneeded),
        }
    }

    async fn assess_questions(
        &self,
        questions: &[Question<'_>],
        state: &serde_json::Value,
    ) -> Result<DecisionBatch, ModelError> {
        match self {
            Self::Jev(jev) => Ok(jev.assess_questions(questions, state).await?),
            Self::Unneeded => Err(ModelError::Unneeded),
        }
    }
}
