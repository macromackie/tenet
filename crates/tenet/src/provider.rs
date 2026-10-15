//! The model a check sends requests to, and what kind of failure a Jev request met.

use anyhow::{Result, bail};
use ev_grep_jev::Jev;
use tenet_engine::{Assessment, Candidate, DecisionBatch, Evaluator, FailureKind, Question};

/// The provider's evaluator, or none for a check whose contracts can send no request.
pub(crate) enum Model {
    Jev(Box<Jev>),
    Unneeded,
}

impl Evaluator for Model {
    async fn assess(&self, query: &str, candidate: &Candidate) -> Result<Assessment> {
        match self {
            Self::Jev(jev) => jev.assess(query, candidate).await,
            Self::Unneeded => bail!("this check has no contract that sends a model request"),
        }
    }

    async fn assess_questions(
        &self,
        questions: &[Question<'_>],
        state: &serde_json::Value,
    ) -> Result<DecisionBatch> {
        match self {
            Self::Jev(jev) => jev.assess_questions(questions, state).await,
            Self::Unneeded => bail!("this check has no contract that sends a model request"),
        }
    }
}

/// The kind of failure a Jev error reports. The pinned ev-grep-jev keeps its HTTP status and
/// transport errors private and reports them only as text, so this reads its messages; only a
/// timeout arrives as a typed error. A message it does not recognize is `Unknown`.
pub(crate) fn classify(error: &anyhow::Error) -> FailureKind {
    if error
        .downcast_ref::<tokio::time::error::Elapsed>()
        .is_some()
    {
        return FailureKind::Timeout;
    }
    let message = error.to_string();
    let status = message
        .strip_prefix("Jev returned HTTP ")
        .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|digits| digits.parse::<u16>().ok());
    match status {
        Some(408) => FailureKind::Timeout,
        Some(429) => FailureKind::RateLimited,
        Some(499) => FailureKind::Interrupted,
        Some(401 | 402) => FailureKind::Authentication,
        Some(300..=399 | 404 | 405) => FailureKind::Configuration,
        Some(413) => FailureKind::InputTooLarge,
        Some(500..=599) => FailureKind::ServerError,
        Some(_) => FailureKind::InvalidRequest,
        None if message.starts_with("Jev transport failed") => FailureKind::Connection,
        None if message.starts_with("encoded request exceeds")
            || message.starts_with("encoded state exceeds") =>
        {
            FailureKind::InputTooLarge
        }
        None => FailureKind::Unknown,
    }
}
