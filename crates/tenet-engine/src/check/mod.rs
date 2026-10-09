mod assess;
mod conclusion;
mod context;
mod prompt;

use crate::{Event, EvidencePacket, Plan, Summary};
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

pub async fn run(
    plan: &Plan,
    options: Options,
    evaluator: &impl Evaluator,
    emit: &impl Fn(Event) -> Result<()>,
) -> Result<Summary> {
    ensure!(
        (1..=256).contains(&options.jobs) && options.max_requests > 0,
        "jobs must be between 1 and 256 and max-requests must be positive"
    );
    ensure!(
        options.min_confidence.is_finite() && (0.0..=1.0).contains(&options.min_confidence),
        "min-confidence must be between 0 and 1"
    );
    let started = Instant::now();
    let evidence = plan.capture().await?;
    let requests = Cell::new(0);
    let slots = Semaphore::new(options.jobs);
    let shared = Pool {
        evidence: &evidence,
        options,
        evaluator,
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
    while let Some(result) = running.next().await {
        let (index, (summary, conclusion)) = result?;
        total.merge(&summary);
        ready.insert(index, (summary, conclusion));
        while let Some((summary, conclusion)) = ready.remove(&next) {
            emit(Event::ContractFinished {
                contract: plan.contracts[next].name.clone(),
                summary,
                conclusion: Some(Box::new(conclusion)),
            })?;
            next += 1;
        }
    }
    total.requests = requests.get();
    total.elapsed_ms = started.elapsed().as_millis();
    Ok(total)
}

struct Pool<'a, E, F> {
    evidence: &'a EvidencePacket<'a>,
    options: Options,
    evaluator: &'a E,
    emit: &'a F,
    requests: &'a Cell<usize>,
    slots: &'a Semaphore,
}

impl<E: Evaluator, F: Fn(Event) -> Result<()>> Pool<'_, E, F> {
    async fn contract(&self, contract: &Contract) -> Result<(Summary, crate::ContractResult)> {
        let started = Instant::now();
        let local_requests = Cell::new(0);
        let subjects: Vec<_> = self
            .evidence
            .plan
            .files
            .iter()
            .filter(|p| self.evidence.plan.in_scope(p, &contract.scope))
            .collect();
        (self.emit)(Event::ContractStarted {
            contract: contract.name.clone(),
            path: contract.path.display().to_string(),
            total: subjects.len(),
        })?;
        let prepared = match context::prepare(self.evidence, contract) {
            context::Preparation::Ready(context) => context,
            context::Preparation::Unresolved(conclusion) => {
                let mut summary = Summary::default();
                summary.conclude(&conclusion);
                summary.elapsed_ms = started.elapsed().as_millis();
                return Ok((summary, *conclusion));
            }
        };
        let context = assess::Context {
            evidence: self.evidence,
            contract,
            evaluator: self.evaluator,
            emit: self.emit,
            requests: self.requests,
            max_requests: self.options.max_requests,
            min_confidence: self.options.min_confidence,
            local_requests: &local_requests,
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
            results: &results,
            summary: &summary,
            requests: self.requests,
            max_requests: self.options.max_requests,
            min_confidence: self.options.min_confidence,
            local_requests: &local_requests,
        }
        .assess(prepared, self.evaluator, self.emit)
        .await?;
        summary.conclude(&conclusion);
        summary.requests = local_requests.get();
        summary.elapsed_ms = started.elapsed().as_millis();
        Ok((summary, conclusion))
    }
}
