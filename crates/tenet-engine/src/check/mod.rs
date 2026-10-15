mod assess;
mod conclusion;
pub(crate) mod context;
mod prompt;

use crate::{
    CheckResult, ContractResult, ContractState, Event, EvidencePacket, Summary,
    review::{Answer, Checked, Concluded, Conclusion, FileResult, Review},
};
use anyhow::{Result, ensure};
use ev_grep_core::Evaluator;
use futures_util::{StreamExt, stream};
use std::{cell::Cell, collections::BTreeMap, time::Instant};
use tenet_contracts::Contract;
use tokio::sync::Semaphore;

#[derive(Clone, Copy)]
pub struct Options {
    pub jobs: usize,
    pub max_requests: usize,
    pub min_confidence: f64,
}

/// Assess every planned contract against captured evidence. Answers to requests identical to
/// earlier ones come from the review's cache, and each conclusion is combined with the review's
/// records into the contract's state.
pub async fn run(
    evidence: &EvidencePacket<'_>,
    options: Options,
    evaluator: &impl Evaluator,
    review: &Review,
    emit: &impl Fn(Event) -> Result<()>,
) -> Result<Checked> {
    ensure!(
        (1..=256).contains(&options.jobs) && options.max_requests > 0,
        "jobs must be between 1 and 256 and max-requests must be positive"
    );
    ensure!(
        options.min_confidence.is_finite() && (0.0..=1.0).contains(&options.min_confidence),
        "min-confidence must be between 0 and 1"
    );
    let plan = evidence.plan;
    let requests = Cell::new(0);
    let slots = Semaphore::new(options.jobs);
    let shared = Pool {
        evidence,
        options,
        evaluator,
        review,
        emit,
        requests: &requests,
        slots: &slots,
    };
    let shared = &shared;
    let mut running = stream::iter(plan.contracts.iter().enumerate())
        .map(|(index, contract)| async move {
            Ok::<_, anyhow::Error>((index, shared.contract(contract).await?))
        })
        .buffer_unordered(options.jobs);
    let mut ready = BTreeMap::new();
    let mut next = 0;
    let mut total = Summary::default();
    let mut contracts = Vec::new();
    while let Some(result) = running.next().await {
        let (index, assessed) = result?;
        total.merge(&assessed.summary);
        ready.insert(index, assessed);
        while let Some(assessed) = ready.remove(&next) {
            let contract = &plan.contracts[next];
            let result = &assessed.conclusion;
            contracts.push(Concluded {
                contract: contract.name.clone(),
                input: result.input.clone(),
                scope: result.scope,
                conclusion: Some(Conclusion {
                    status: result.status,
                    reason_code: result.reason_code,
                    reason: result.reason.clone(),
                    violation: result.violation,
                    answer: result.assessment.as_ref().map(|answer| Answer {
                        choice: answer.choice,
                        confidence: answer.confidence,
                    }),
                    error: result.error.clone(),
                    conflicting_files: result.conflicting_files.clone(),
                }),
                carried_from: None,
                results: assessed
                    .results
                    .iter()
                    .map(|result| FileResult {
                        path: result.path.clone(),
                        status: result.status,
                        violation: result.verification.as_ref().map(crate::violation),
                        omitted: result.omitted.clone(),
                    })
                    .collect(),
                state: assessed.state.clone(),
            });
            emit(Event::ContractFinished {
                contract: contract.name.clone(),
                summary: assessed.summary,
                conclusion: Some(Box::new(assessed.conclusion)),
                state: assessed.state,
            })?;
            next += 1;
        }
    }
    total.requests = requests.get();
    Ok(Checked {
        summary: total,
        contracts,
    })
}

/// Reuse earlier conclusions for unchanged inputs without asking a model, reporting each contract
/// as `run` does. A reused conclusion counts as answered from an earlier check.
pub fn carry(
    evidence: &EvidencePacket<'_>,
    review: &Review,
    emit: &impl Fn(Event) -> Result<()>,
) -> Result<Checked> {
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
                .filter(|p| plan.governs(contract, p))
                .count(),
        })?;
        let input = evidence.input_key(contract);
        let scope = plan.requested_scope(contract);
        let earlier = review
            .assessment(&contract.name, &input)
            .filter(|a| a.conclusion.is_some());
        let conclusion = earlier.and_then(|a| a.conclusion.clone());
        let state = review.state(
            plan,
            contract,
            &input,
            conclusion.as_ref().map(|c| c.status),
        );
        let result = conclusion.as_ref().map(|c| ContractResult {
            status: c.status,
            scope,
            input: input.clone(),
            violation: c.violation,
            reason: c.reason.clone(),
            reason_code: c.reason_code,
            context: None,
            assessment: None,
            routing: None,
            evidence_hash: None,
            error: c.error.clone(),
            conflicting_files: c.conflicting_files.clone(),
            request: None,
            cached: true,
        });
        summary.cached += usize::from(result.is_some());
        emit(Event::ContractFinished {
            contract: contract.name.clone(),
            summary: Summary::default(),
            conclusion: result.map(Box::new),
            state: state.clone(),
        })?;
        contracts.push(Concluded {
            contract: contract.name.clone(),
            input,
            scope,
            conclusion,
            carried_from: earlier.map(|a| a.carried_from.unwrap_or(a.rev)),
            results: earlier.map(|a| a.results.clone()).unwrap_or_default(),
            state,
        });
    }
    Ok(Checked { summary, contracts })
}

struct Assessed {
    summary: Summary,
    conclusion: ContractResult,
    results: Vec<CheckResult>,
    state: ContractState,
}

struct Pool<'a, E, F> {
    evidence: &'a EvidencePacket<'a>,
    options: Options,
    evaluator: &'a E,
    review: &'a Review,
    emit: &'a F,
    requests: &'a Cell<usize>,
    slots: &'a Semaphore,
}

impl<E: Evaluator, F: Fn(Event) -> Result<()>> Pool<'_, E, F> {
    async fn contract(&self, contract: &Contract) -> Result<Assessed> {
        let started = Instant::now();
        let local_requests = Cell::new(0);
        let refused = Cell::new(0);
        let input = self.evidence.input_key(contract);
        let subjects: Vec<_> = self
            .evidence
            .plan
            .files
            .iter()
            .filter(|p| self.evidence.plan.governs(contract, p))
            .collect();
        (self.emit)(Event::ContractStarted {
            contract: contract.name.clone(),
            path: contract.path.display().to_string(),
            total: subjects.len(),
        })?;
        let prepared = context::prepare(self.evidence, contract);
        let context = assess::Context {
            evidence: self.evidence,
            contract,
            evaluator: self.evaluator,
            answers: &self.review.answers,
            emit: self.emit,
            requests: self.requests,
            max_requests: self.options.max_requests,
            min_confidence: self.options.min_confidence,
            local_requests: &local_requests,
            refused: &refused,
        };
        let mut running = stream::iter(subjects)
            .map(|path| async {
                let _permit = self.slots.acquire().await?;
                assess::assess(&context, path).await
            })
            .buffer_unordered(self.options.jobs);
        let mut summary = Summary::default();
        let mut results = Vec::new();
        while let Some(result) = running.next().await {
            let result = result?;
            summary.add(&result);
            (self.emit)(Event::Result {
                result: Box::new(result.clone()),
            })?;
            results.push(result);
        }
        let _permit = self.slots.acquire().await?;
        let conclusion = conclusion::Completion {
            evidence: self.evidence,
            contract,
            input: &input,
            answers: &self.review.answers,
            results: &results,
            summary: &summary,
            requests: self.requests,
            max_requests: self.options.max_requests,
            min_confidence: self.options.min_confidence,
            local_requests: &local_requests,
            refused: &refused,
        }
        .assess(prepared, self.evaluator, self.emit)
        .await?;
        summary.conclude(&conclusion);
        summary.requests = local_requests.get();
        summary.over_budget = refused.get();
        summary.elapsed_ms = started.elapsed().as_millis();
        let state = self.review.state(
            self.evidence.plan,
            contract,
            &input,
            Some(conclusion.status),
        );
        Ok(Assessed {
            summary,
            conclusion,
            results,
            state,
        })
    }
}
