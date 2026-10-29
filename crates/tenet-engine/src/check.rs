mod assess;
mod conclusion;
pub(crate) mod context;
mod predict;
mod prompt;

use alloc::collections::BTreeMap;
use core::cell::Cell;
use std::time::Instant;

pub use context::CombinedError;
use ev_grep_core::Evaluator;
use futures_util::{StreamExt as _, stream};
pub use predict::{Prediction, estimate};
use tenet_contracts::Contract;
use tokio::sync::{AcquireError, Semaphore, SemaphorePermit};
use tracing::Instrument as _;

use crate::{
    CheckResult,
    ClassifyRequest,
    ContractResult,
    ContractState,
    Event,
    EvidencePacket,
    RequestFailure,
    Summary,
    review::{Answer, Checked, Concluded, Conclusion, FileResult, Review, StoreError},
};

/// Why a check stopped before concluding every contract. A failure of the caller's `emit`
/// callback ends the check as the caller's own error instead.
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error(transparent)]
    Cache(#[from] StoreError),
    /// A combined assessment failed in a way every later request would.
    #[error("{reason:?}, while assessing {contract:?}")]
    CombinedRequest {
        failure: RequestFailure,
        reason: &'static str,
        contract: String,
        source: anyhow::Error,
    },
    /// A file's request failed in a way every later request would; `failure` names its kind.
    #[error("{reason:?}, while checking {path:?} against {contract:?}")]
    FileRequest {
        failure: RequestFailure,
        reason: &'static str,
        path: String,
        contract: String,
        source: anyhow::Error,
    },
    #[error("min-confidence must be between 0 and 1")]
    InvalidConfidence,
    #[error("jobs must be between 1 and 256 and max-requests must be positive")]
    InvalidJobs,
    #[error(transparent)]
    Slots(#[from] AcquireError),
}

/// How a check runs: its parallel jobs, request budget, and confidence threshold.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub jobs: usize,
    pub max_requests: usize,
    pub min_confidence: f64,
}

/// Assess every planned contract against captured evidence.
///
/// Answers to requests identical to earlier ones come from the review's cache, and each conclusion
/// is combined with the review's records into the contract's state. An `emit` failure returns
/// unchanged.
#[expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "`next` stays below `plan.contracts.len()`, which the loop checks before each index, \
              and the counters are bounded by the contract count."
)]
#[inline]
#[expect(
    clippy::future_not_send,
    reason = "The checker drives this future on one task and never moves it across threads."
)]
#[expect(
    clippy::impl_trait_in_params,
    reason = "Callers pass closures and evaluators whose types they cannot name."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
pub async fn run<Halted>(
    evidence: &EvidencePacket<'_>,
    options: Options,
    evaluator: &impl Evaluator<Error: ClassifyRequest>,
    review: &Review,
    emit: &impl Fn(Event) -> Result<(), Halted>,
) -> Result<Checked, Halted>
where
    Halted: From<CheckError>,
{
    if !(1..=256).contains(&options.jobs) || options.max_requests == 0 {
        return Err(CheckError::InvalidJobs.into());
    }
    if !(options.min_confidence.is_finite()
        && (0.0_f64..=1.0_f64).contains(&options.min_confidence))
    {
        return Err(CheckError::InvalidConfidence.into());
    }
    let plan = evidence.plan;
    let requests = Cell::new(0);
    let slots = Semaphore::new(options.jobs);
    let shared = Pool {
        emit,
        evaluator,
        evidence,
        options,
        requests: &requests,
        review,
        slots: &slots,
    };
    let borrowed = &shared;
    let mut running = stream::iter(plan.contracts.iter().enumerate())
        .map(async move |(index, contract)| {
            let span = tracing::info_span!("tenet.contract", contract = contract.name.as_str());
            let assessed = borrowed.assess_contract(contract).instrument(span).await?;
            Ok::<_, Halted>((index, assessed))
        })
        .buffer_unordered(options.jobs);
    let mut ready_contracts = BTreeMap::new();
    let mut next = 0;
    let mut total = Summary::default();
    let mut contracts = Vec::new();
    while let Some(result) = running.next().await {
        let (index, assessed) = result?;
        total.merge(&assessed.summary);
        drop(ready_contracts.insert(index, assessed));
        while let Some(ordered) = ready_contracts.remove(&next) {
            let contract = &plan.contracts[next];
            let settled = &ordered.conclusion;
            contracts.push(Concluded {
                carried_from: None,
                conclusion: Some(Conclusion {
                    status: settled.status,
                    reason_code: settled.reason_code,
                    reason: settled.reason.clone(),
                    violation: settled.violation,
                    answer: settled.assessment.as_ref().map(|answer| {
                        Answer {
                            choice: answer.choice,
                            confidence: answer.confidence,
                        }
                    }),
                    error: settled.error.clone(),
                    conflicting_files: settled.conflicting_files.clone(),
                }),
                contract: contract.name.clone(),
                input: settled.input.clone(),
                results: ordered
                    .results
                    .iter()
                    .map(|file_result| {
                        FileResult {
                            path: file_result.path.clone(),
                            status: file_result.status,
                            violation: file_result
                                .verification
                                .as_ref()
                                .map(crate::estimate_violation),
                            omitted: file_result.omitted.clone(),
                        }
                    })
                    .collect(),
                scope: settled.scope,
                state: ordered.state.clone(),
            });
            emit(Event::ContractFinished {
                contract: contract.name.clone(),
                summary: ordered.summary,
                conclusion: Some(Box::new(ordered.conclusion)),
                state: ordered.state,
            })?;
            next += 1;
        }
    }
    total.requests = requests.get();
    Ok(Checked {
        contracts,
        summary: total,
    })
}

/// Reuse earlier conclusions for unchanged inputs without asking a model, reporting each contract
/// as `run` does. A reused conclusion counts as answered from an earlier check.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Counts of results, contracts and requests are bounded by the repository's files and \
              the request budget, far below the integer's limit."
)]
#[inline]
#[expect(
    clippy::impl_trait_in_params,
    reason = "Callers pass closures and evaluators whose types they cannot name."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
pub fn apply_review<Halted>(
    evidence: &EvidencePacket<'_>,
    review: &Review,
    emit: &impl Fn(Event) -> Result<(), Halted>,
) -> Result<Checked, Halted> {
    let plan = evidence.plan;
    let mut summary = Summary::default();
    let mut contracts = Vec::new();
    for contract in &plan.contracts {
        emit(Event::ContractStarted {
            contract: contract.name.clone(),
            path: contract.path.display().to_string(),
            total: plan
                .files
                .iter()
                .filter(|path| plan.is_governing(contract, path))
                .count(),
        })?;
        let input = evidence.derive_input_key(contract);
        let scope = plan.find_requested_scope(contract);
        let earlier = review
            .find_assessment(&contract.name, &input)
            .filter(|assessment| assessment.conclusion.is_some());
        let conclusion = earlier.and_then(|assessment| assessment.conclusion.clone());
        let state = review.compute_state(
            plan,
            contract,
            &input,
            conclusion.as_ref().map(|concluded| concluded.status),
        );
        let result = conclusion.as_ref().map(|concluded| {
            ContractResult {
                status: concluded.status,
                scope,
                input: input.clone(),
                violation: concluded.violation,
                reason: concluded.reason.clone(),
                reason_code: concluded.reason_code,
                context: None,
                assessment: None,
                routing: None,
                evidence_hash: None,
                error: concluded.error.clone(),
                failure: None,
                conflicting_files: concluded.conflicting_files.clone(),
                request: None,
                cached: true,
            }
        });
        summary.cached += usize::from(result.is_some());
        emit(Event::ContractFinished {
            contract: contract.name.clone(),
            summary: Summary::default(),
            conclusion: result.map(Box::new),
            state: state.clone(),
        })?;
        contracts.push(Concluded {
            carried_from: earlier
                .map(|assessment| assessment.carried_from.unwrap_or(assessment.rev)),
            conclusion,
            contract: contract.name.clone(),
            input,
            results: earlier
                .map(|assessment| assessment.results.clone())
                .unwrap_or_default(),
            scope,
            state,
        });
    }
    Ok(Checked { contracts, summary })
}

struct Assessed {
    conclusion: ContractResult,
    results: Vec<CheckResult>,
    state: ContractState,
    summary: Summary,
}

struct Pool<'plan, Assessor, Emit> {
    emit: &'plan Emit,
    evaluator: &'plan Assessor,
    evidence: &'plan EvidencePacket<'plan>,
    options: Options,
    requests: &'plan Cell<usize>,
    review: &'plan Review,
    slots: &'plan Semaphore,
}

impl<
    Assessor: Evaluator<Error: ClassifyRequest>,
    Emit: Fn(Event) -> Result<(), Halted>,
    Halted: From<CheckError>,
> Pool<'_, Assessor, Emit>
{
    /// Wait in a `tenet.wait` span for one of the check's parallel slots.
    #[expect(
        clippy::future_not_send,
        reason = "The checker drives this future on one task and never moves it across threads."
    )]
    async fn acquire_slot(&self) -> Result<SemaphorePermit<'_>, CheckError> {
        let span = tracing::info_span!("tenet.wait");
        Ok(self.slots.acquire().instrument(span).await?)
    }

    #[expect(
        clippy::future_not_send,
        reason = "The checker drives this future on one task and never moves it across threads."
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "The steps run in one order and share local state; splitting them would add \
                  forwarding functions."
    )]
    async fn assess_contract(&self, contract: &Contract) -> Result<Assessed, Halted> {
        let started = Instant::now();
        let local_requests = Cell::new(0);
        let refused = Cell::new(0);
        let input = self.evidence.derive_input_key(contract);
        let subjects: Vec<_> = self
            .evidence
            .plan
            .files
            .iter()
            .filter(|path| self.evidence.plan.is_governing(contract, path))
            .collect();
        (self.emit)(Event::ContractStarted {
            contract: contract.name.clone(),
            path: contract.path.display().to_string(),
            total: subjects.len(),
        })?;
        let prepared = context::prepare(self.evidence, contract);
        let context = assess::Context {
            answers: &self.review.answers,
            contract,
            emit: self.emit,
            evaluator: self.evaluator,
            evidence: self.evidence,
            local_requests: &local_requests,
            max_requests: self.options.max_requests,
            min_confidence: self.options.min_confidence,
            refused: &refused,
            requests: self.requests,
        };
        let mut running = stream::iter(subjects)
            .map(async |path| {
                let span = tracing::info_span!(
                    "tenet.pair",
                    contract = contract.name.as_str(),
                    path = %path.display(),
                );
                async {
                    let permit = self.acquire_slot().await?;
                    // A buffered failure keeps its permit until the caller can stop queued work.
                    Ok::<_, Halted>((permit, assess::assess(&context, path).await))
                }
                .instrument(span)
                .await
            })
            .buffer_unordered(self.options.jobs);
        let mut summary = Summary::default();
        let mut results = Vec::new();
        while let Some(completed) = running.next().await {
            let (_permit, result) = completed?;
            let outcome = result?;
            summary.add(&outcome);
            (self.emit)(Event::Result {
                result: Box::new(outcome.clone()),
            })?;
            results.push(outcome);
        }
        let _permit = self.acquire_slot().await?;
        let conclusion = conclusion::Completion {
            answers: &self.review.answers,
            contract,
            evidence: self.evidence,
            input: &input,
            local_requests: &local_requests,
            max_requests: self.options.max_requests,
            min_confidence: self.options.min_confidence,
            refused: &refused,
            requests: self.requests,
            results: &results,
            summary: &summary,
        }
        .assess(prepared, self.evaluator, self.emit)
        .await?;
        summary.record_conclusion(&conclusion);
        summary.requests = local_requests.get();
        summary.over_budget = refused.get();
        summary.elapsed_ms = started.elapsed().as_millis();
        let state = self.review.compute_state(
            self.evidence.plan,
            contract,
            &input,
            Some(conclusion.status),
        );
        Ok(Assessed {
            conclusion,
            results,
            state,
            summary,
        })
    }
}
