mod assess;
mod conclusion;
mod context;
mod prompt;

use crate::{Event, Plan, Summary};
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
    let started = Instant::now();
    let requests = Cell::new(0);
    let slots = Semaphore::new(options.jobs);
    let shared = Pool {
        plan,
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
    plan: &'a Plan,
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
            .plan
            .files
            .iter()
            .filter(|p| self.plan.in_scope(p, &contract.scope))
            .collect();
        (self.emit)(Event::ContractStarted {
            contract: contract.name.clone(),
            path: contract.path.display().to_string(),
            total: subjects.len(),
        })?;
        let context = assess::Context {
            plan: self.plan,
            contract,
            evaluator: self.evaluator,
            emit: self.emit,
            requests: self.requests,
            max_requests: self.options.max_requests,
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
            plan: self.plan,
            contract,
            results: &results,
            summary: &summary,
            requests: self.requests,
            max_requests: self.options.max_requests,
            local_requests: &local_requests,
        }
        .assess(self.evaluator, self.emit)
        .await?;
        summary.conclude(&conclusion);
        summary.requests = local_requests.get();
        summary.elapsed_ms = started.elapsed().as_millis();
        Ok((summary, conclusion))
    }
}
