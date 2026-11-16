use alloc::collections::BTreeMap;
use core::cell::{Cell, RefCell};

use ev_grep_core::{Assessment, Candidate, DecisionBatch, ErrorKind, Evaluator, Failure, Question};

use super::CheckError;
use crate::{
    CheckResult,
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
#[derive(Clone, Debug, Default)]
pub struct Prediction {
    pub cached: usize,
    pub cached_file_pairs: usize,
    pub combined_requests: usize,
    pub contract_requests: BTreeMap<String, usize>,
    pub file_pairs: usize,
    pub file_requests: usize,
}

/// Follow `run` without sending anything.
///
/// An evaluator that answers no request stands in for the provider, so cached answers, inputs that
/// send nothing, and unaffected contracts count exactly as a check counts them. The budget is
/// unlimited; the caller compares the total with its own.
#[inline]
#[expect(
    clippy::future_not_send,
    reason = "The checker drives this future on one task and never moves it across threads."
)]
pub async fn estimate(
    evidence: &EvidencePacket<'_>,
    review: &Review,
    jobs: usize,
    contract_batch_size: usize,
) -> Result<Prediction, CheckError<PredictionError>> {
    estimate_planned(evidence, review, jobs, contract_batch_size, None).await
}

/// Predict the same deterministic packs using the provider's actual encoded request size.
#[inline]
#[expect(
    clippy::future_not_send,
    reason = "The checker drives this future on one task and never moves it across threads."
)]
pub async fn estimate_with_measure<Measure>(
    evidence: &EvidencePacket<'_>,
    review: &Review,
    jobs: usize,
    contract_batch_size: usize,
    measure: &Measure,
) -> Result<Prediction, CheckError<PredictionError>>
where
    Measure: Fn(&[Question<'_>], &serde_json::Value) -> Result<usize, PredictionError> + Sync,
{
    estimate_planned(evidence, review, jobs, contract_batch_size, Some(measure)).await
}

type Measure<'measure> =
    dyn Fn(&[Question<'_>], &serde_json::Value) -> Result<usize, PredictionError> + Sync + 'measure;

#[expect(
    clippy::arithmetic_side_effects,
    reason = "Captured files and the request budget bound standalone resume caps."
)]
fn count_standalone_requests(result: &CheckResult) -> usize {
    if let Some(summary) = &result.hunk_summary {
        return summary.requests + summary.cached;
    }
    if result.shared_request.is_some() {
        return 1;
    }
    result.requests + usize::from(result.cached)
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "The repository's files and contracts bound predicted request counts."
)]
#[expect(
    clippy::future_not_send,
    reason = "The checker drives this future on one task and never moves it across threads."
)]
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "Other progress events do not submit requests or change standalone resume caps."
)]
#[expect(
    clippy::too_many_lines,
    reason = "One event observer keeps physical dispatch counts and conservative resume caps \
              together."
)]
async fn estimate_planned(
    evidence: &EvidencePacket<'_>,
    review: &Review,
    jobs: usize,
    contract_batch_size: usize,
    measure: Option<&Measure<'_>>,
) -> Result<Prediction, CheckError<PredictionError>> {
    let options = super::Options {
        contract_batch_size,
        jobs,
        max_requests: usize::MAX,
        min_confidence: MIN_CONFIDENCE,
    };
    let (file_requests, combined_requests) = (Cell::new(0), Cell::new(0));
    let contract_requests = RefCell::new(BTreeMap::new());
    let file_pairs = Cell::new(0);
    let cached_file_pairs = Cell::new(0);
    let checked = super::run(evidence, options, &Unsent { measure }, review, &|event| {
        match event {
            Event::CheckStarted { .. } => file_pairs.set(file_pairs.get() + 1),
            Event::Result { result } => {
                cached_file_pairs.set(cached_file_pairs.get() + usize::from(result.cached));
                let needed = count_standalone_requests(&result);
                let mut counts = contract_requests.borrow_mut();
                *counts.entry(result.contract).or_default() += needed;
            },
            Event::FileBatchStarted { .. }
            | Event::StageStarted {
                stage: Stage::Applicability,
                ..
            } => {
                file_requests.set(file_requests.get() + 1);
            },
            Event::StageStarted {
                stage: Stage::Completeness,
                contract,
                ..
            } => {
                combined_requests.set(combined_requests.get() + 1);
                *contract_requests.borrow_mut().entry(contract).or_default() += 1;
            },
            Event::ContractFinished {
                contract,
                conclusion,
                ..
            } => {
                if conclusion.is_some_and(|settled| settled.cached) {
                    *contract_requests.borrow_mut().entry(contract).or_default() += 1;
                }
            },
            _ => {},
        }
        Ok::<_, CheckError<PredictionError>>(())
    })
    .await?;
    Ok(Prediction {
        cached: checked.summary.cached,
        cached_file_pairs: cached_file_pairs.get(),
        combined_requests: combined_requests.get(),
        contract_requests: contract_requests.into_inner(),
        file_pairs: file_pairs.get(),
        file_requests: file_requests.get(),
    })
}

struct Unsent<'measure> {
    measure: Option<&'measure Measure<'measure>>,
}

/// Why a dry run's evaluator answered nothing.
#[derive(Debug, thiserror::Error)]
pub enum PredictionError {
    /// Preparing the evaluator's input failed.
    #[error(transparent)]
    Core(#[from] ev_grep_core::InputError),
    /// The provider could not measure its encoded request.
    #[error("request measurement failed: {0:#}")]
    Measurement(String),
    /// A dry run counts requests without sending them.
    #[error("a dry run sends no request")]
    Unsent,
}

impl Failure for PredictionError {
    #[inline]
    fn classify(&self) -> ErrorKind {
        match self {
            Self::Core(error) => error.classify(),
            Self::Unsent | Self::Measurement(_) => ErrorKind::Configuration,
        }
    }
}

impl ClassifyRequest for PredictionError {
    #[inline]
    fn classify_request(&self) -> RequestFailure {
        RequestFailure::new(FailureKind::Unknown, false)
    }
}

impl Evaluator for Unsent<'_> {
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

    fn measure_questions(
        &self,
        questions: &[Question<'_>],
        state: &serde_json::Value,
    ) -> Result<usize, PredictionError> {
        if let Some(measure) = self.measure {
            return measure(questions, state);
        }
        let named: Vec<_> = questions
            .iter()
            .map(|question| [question.id, question.query])
            .collect();
        let request = serde_json::json!({ "questions": named, "state": state });
        serde_json::to_vec(&request)
            .map(|bytes| bytes.len())
            .map_err(|error| PredictionError::Measurement(error.to_string()))
    }
}
