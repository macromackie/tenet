use core::cell::Cell;

use ev_grep_core::{Assessment, Candidate, DecisionBatch, ErrorKind, Evaluator, Failure, Question};

use super::CheckError;
use crate::{
    ClassifyRequest,
    Event,
    EvidencePacket,
    FailureKind,
    MIN_CONFIDENCE,
    RequestFailure,
    Review,
    Stage,
};

/// The model requests a check would send, and those the review's answers would answer instead.
#[derive(Clone, Copy, Debug, Default)]
pub struct Prediction {
    pub cached: usize,
    pub combined_requests: usize,
    pub file_requests: usize,
}

/// Follow `run` without sending anything.
///
/// An evaluator that answers no request stands in for the provider, so cached answers, inputs that
/// send nothing, and unaffected contracts count exactly as a check counts them. The budget is
/// unlimited; the caller compares the total with its own.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Counts of results, contracts and requests are bounded by the repository's files and \
              the request budget, far below the integer's limit."
)]
#[inline]
#[expect(
    clippy::future_not_send,
    reason = "The checker drives this future on one task and never moves it across threads."
)]
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "The remaining variants take no action here, and listing each would repeat the enum."
)]
pub async fn estimate(
    evidence: &EvidencePacket<'_>,
    review: &Review,
    jobs: usize,
) -> Result<Prediction, CheckError<PredictionError>> {
    let options = super::Options {
        jobs,
        max_requests: usize::MAX,
        min_confidence: MIN_CONFIDENCE,
    };
    let (file_requests, combined_requests) = (Cell::new(0), Cell::new(0));
    let checked = super::run(evidence, options, &Unsent, review, &|event| {
        match event {
            Event::StageStarted {
                stage: Stage::Completeness,
                ..
            } => combined_requests.set(combined_requests.get() + 1),
            Event::StageStarted {
                stage: Stage::Applicability,
                ..
            } => file_requests.set(file_requests.get() + 1),
            _ => {},
        }
        Ok::<_, CheckError<PredictionError>>(())
    })
    .await?;
    Ok(Prediction {
        cached: checked.summary.cached,
        combined_requests: combined_requests.get(),
        file_requests: file_requests.get(),
    })
}

struct Unsent;

/// Why a dry run's evaluator answered nothing.
#[derive(Debug, thiserror::Error)]
pub enum PredictionError {
    /// Preparing the evaluator's input failed.
    #[error(transparent)]
    Core(#[from] ev_grep_core::InputError),
    /// A dry run counts requests without sending them.
    #[error("a dry run sends no request")]
    Unsent,
}

impl Failure for PredictionError {
    #[inline]
    fn classify(&self) -> ErrorKind {
        match self {
            Self::Core(error) => error.classify(),
            Self::Unsent => ErrorKind::Configuration,
        }
    }
}

impl ClassifyRequest for PredictionError {
    #[inline]
    fn classify_request(&self) -> RequestFailure {
        RequestFailure::new(FailureKind::Unknown, false)
    }
}

impl Evaluator for Unsent {
    type Error = PredictionError;

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "The trait is async for evaluators that wait on a model."
    )]
    async fn assess(&self, _: &str, _: &Candidate) -> Result<Assessment, PredictionError> {
        Err(PredictionError::Unsent)
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "The trait is async for evaluators that wait on a model."
    )]
    async fn assess_questions(
        &self,
        _: &[Question<'_>],
        _: &serde_json::Value,
    ) -> Result<DecisionBatch, PredictionError> {
        Err(PredictionError::Unsent)
    }
}
