mod assess;
mod batch;
mod conclusion;
mod context;
mod error;
mod options;
mod predict;
mod prompt;
mod record;
mod reuse;

use alloc::collections::BTreeMap;
use core::cell::Cell;
use std::time::Instant;

pub use error::CheckError;
use ev_grep_core::Evaluator;
use futures_util::{StreamExt as _, stream};
pub use options::Options;
pub use predict::{Prediction, PredictionError, estimate, estimate_with_measure};
pub use reuse::apply_review;
use tenet_contracts::Contract;
use tokio::sync::{Semaphore, SemaphorePermit};
use tracing::Instrument as _;

use crate::{
    CheckResult,
    ClassifyRequest,
    ContractResult,
    ContractState,
    Event,
    EvidencePacket,
    Summary,
    review::{Checked, Review},
    telemetry,
};

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
pub async fn run<Assessor, Halted>(
    evidence: &EvidencePacket<'_>,
    options: Options,
    evaluator: &Assessor,
    review: &Review,
    emit: &impl Fn(Event) -> Result<(), Halted>,
) -> Result<Checked, Halted>
where
    Assessor: Evaluator<Error: ClassifyRequest>,
    Halted: From<CheckError<Assessor::Error>>,
{
    if !(1..=256).contains(&options.jobs) || options.max_requests == 0 {
        return Err(CheckError::InvalidJobs.into());
    }
    if !(options.min_confidence.is_finite()
        && (0.0_f64..=1.0_f64).contains(&options.min_confidence))
    {
        return Err(CheckError::InvalidConfidence.into());
    }
    if !(1..=64).contains(&options.contract_batch_size) {
        return Err(CheckError::InvalidBatchSize.into());
    }
    let plan = evidence.plan;
    let requests = Cell::new(0);
    let slots = Semaphore::new(options.jobs);
    let pool = Pool {
        batched: None,
        emit,
        evaluator,
        evidence,
        options,
        requests: &requests,
        review,
        slots: &slots,
    };
    let batched = if options.contract_batch_size > 1 {
        Some(batch::prepare(&pool).await?)
    } else {
        None
    };
    let shared = Pool {
        batched: batched.as_ref(),
        ..pool
    };
    let borrowed = &shared;
    let mut running = stream::iter(plan.contracts.iter().enumerate())
        .map(async move |(index, contract)| {
            let span = telemetry::open_contract(&contract.name);
            let assessed = borrowed.assess_contract(contract).instrument(span).await?;
            Ok::<_, Halted>((index, assessed))
        })
        .buffer_unordered(options.jobs);
    let mut ready_contracts = BTreeMap::new();
    let mut next = 0;
    let mut total = batched
        .as_ref()
        .map_or_else(Summary::default, batch::Prepared::read_work);
    let mut contracts = Vec::new();
    while let Some(result) = running.next().await {
        let (index, assessed) = result?;
        total.merge(&assessed.summary);
        drop(ready_contracts.insert(index, assessed));
        while let Some(ordered) = ready_contracts.remove(&next) {
            let contract = &plan.contracts[next];
            contracts.push(record::build_concluded(&ordered, &contract.name));
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

struct Assessed {
    conclusion: ContractResult,
    results: Vec<CheckResult>,
    state: ContractState,
    summary: Summary,
}

struct Pool<'plan, Assessor, Emit> {
    batched: Option<&'plan batch::Prepared>,
    emit: &'plan Emit,
    evaluator: &'plan Assessor,
    evidence: &'plan EvidencePacket<'plan>,
    options: Options,
    requests: &'plan Cell<usize>,
    review: &'plan Review,
    slots: &'plan Semaphore,
}

impl<Assessor, Emit, Halted> Pool<'_, Assessor, Emit>
where
    Assessor: Evaluator<Error: ClassifyRequest>,
    Emit: Fn(Event) -> Result<(), Halted>,
    Halted: From<CheckError<Assessor::Error>>,
{
    /// Wait in a `tenet.wait` span for one of the check's parallel slots.
    #[expect(
        clippy::future_not_send,
        reason = "The checker drives this future on one task and never moves it across threads."
    )]
    async fn acquire_slot(&self) -> Result<SemaphorePermit<'_>, CheckError<Assessor::Error>> {
        let span = telemetry::open_wait();
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
    #[expect(
        clippy::excessive_nesting,
        reason = "The isolated and prefetched branches share finalization, while each file future \
                  retains its permit."
    )]
    async fn assess_contract(&self, contract: &Contract) -> Result<Assessed, Halted> {
        let started = self
            .batched
            .map_or_else(Instant::now, batch::Prepared::read_started);
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
        if self.batched.is_none() {
            (self.emit)(Event::ContractStarted {
                contract: contract.name.clone(),
                path: contract.path.display().to_string(),
                total: subjects.len(),
            })?;
        }
        // Source and inventory are already captured; retain serialized contexts only inside
        // these jobs-bounded contract futures, including when an input exceeds its limit.
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
        let mut summary = Summary::default();
        let mut results = Vec::new();
        if let Some(batch) = self.batched {
            for outcome in batch.read_files(&contract.name) {
                summary.add(outcome);
                results.push(outcome.clone());
            }
            local_requests.set(summary.requests);
        } else {
            let mut running = stream::iter(subjects)
                .map(async |path| {
                    let span = telemetry::open_pair(&contract.name, path);
                    async {
                        let permit = self.acquire_slot().await?;
                        // A buffered failure keeps its permit until the caller can stop queued
                        // work.
                        Ok::<_, Halted>((permit, assess::assess(&context, path).await))
                    }
                    .instrument(span)
                    .await
                })
                .buffer_unordered(self.options.jobs);
            while let Some(completed) = running.next().await {
                let (_permit, result) = completed?;
                let outcome = result?;
                summary.add(&outcome);
                (self.emit)(Event::Result {
                    result: Box::new(outcome.clone()),
                })?;
                results.push(outcome);
            }
        }
        let _permit = self.acquire_slot().await?;
        results.sort_by(|left, right| left.path.cmp(&right.path));
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
            self.evidence.has_complete_identity(contract),
        );
        Ok(Assessed {
            conclusion,
            results,
            state,
            summary,
        })
    }
}
