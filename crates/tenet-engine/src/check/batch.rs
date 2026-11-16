mod dispatch;
mod pack;

use alloc::collections::BTreeMap;
use core::cell::Cell;
use std::{path::Path, time::Instant};

use ev_grep_core::Evaluator;
use futures_util::{StreamExt as _, stream};

use super::{CheckError, Pool, assess};
use crate::{CheckResult, ClassifyRequest, Event, Summary};

pub(super) struct Prepared {
    files: BTreeMap<String, Vec<CheckResult>>,
    started: Instant,
    work: Summary,
}

impl Prepared {
    pub(super) fn read_files(&self, name: &str) -> &[CheckResult] {
        self.files.get(name).map_or(&[], Vec::as_slice)
    }

    pub(super) const fn read_started(&self) -> Instant {
        self.started
    }

    pub(super) const fn read_work(&self) -> Summary {
        self.work
    }
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "The request budget and repository file count bound these work counters."
)]
#[expect(
    clippy::future_not_send,
    reason = "The checker drives this future on one task and never moves it across threads."
)]
// jscpd:ignore-start -- duplication -- Shared checker bounds and start-time capture.
pub(super) async fn prepare<Assessor, Emit, Halted>(
    pool: &Pool<'_, Assessor, Emit>,
) -> Result<Prepared, Halted>
where
    Assessor: Evaluator<Error: ClassifyRequest>,
    Emit: Fn(Event) -> Result<(), Halted>,
    Halted: From<CheckError<Assessor::Error>>,
{
    let started = Instant::now();
    // jscpd:ignore-end
    let mut files = BTreeMap::new();
    for contract in &pool.evidence.plan.contracts {
        let total = pool
            .evidence
            .plan
            .files
            .iter()
            .filter(|path| pool.evidence.plan.is_governing(contract, path))
            .count();
        (pool.emit)(Event::ContractStarted {
            contract: contract.name.clone(),
            path: contract.path.display().to_string(),
            total,
        })?;
        drop(files.insert(contract.name.clone(), Vec::new()));
    }
    let mut running = stream::iter(pool.evidence.plan.files.iter())
        .map(async |path| {
            let _permit = pool.acquire_slot().await?;
            assess_file(pool, path).await
        })
        .buffer_unordered(pool.options.jobs);
    let mut work = Summary::default();
    while let Some(completed) = running.next().await {
        let assessed = completed?;
        work.record_work(&assessed.work);
        work.over_budget += assessed.work.over_budget;
        for result in assessed.results {
            (pool.emit)(Event::Result {
                result: Box::new(result.clone()),
            })?;
            files
                .entry(result.contract.clone())
                .or_default()
                .push(result);
        }
    }
    Ok(Prepared {
        files,
        started,
        work,
    })
}

struct FileAssessment {
    results: Vec<CheckResult>,
    work: Summary,
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "The request budget bounds refused physical requests."
)]
#[expect(
    clippy::future_not_send,
    reason = "The checker drives this future on one task and never moves it across threads."
)]
#[expect(
    clippy::too_many_lines,
    reason = "One ordered loop handles cache reuse, packing and isolated fallback for a file."
)]
async fn assess_file<Assessor, Emit, Halted>(
    pool: &Pool<'_, Assessor, Emit>,
    path: &Path,
) -> Result<FileAssessment, Halted>
where
    Assessor: Evaluator<Error: ClassifyRequest>,
    Emit: Fn(Event) -> Result<(), Halted>,
    Halted: From<CheckError<Assessor::Error>>,
{
    let mut assessed = FileAssessment {
        results: Vec::new(),
        work: Summary::default(),
    };
    let mut pending = pack::Pack::default();
    for contract in pool
        .evidence
        .plan
        .contracts
        .iter()
        .filter(|contract| pool.evidence.plan.is_governing(contract, path))
    {
        let local_requests = Cell::new(0);
        let refused = Cell::new(0);
        let context = assess::Context {
            answers: &pool.review.answers,
            contract,
            emit: pool.emit,
            evaluator: pool.evaluator,
            evidence: pool.evidence,
            local_requests: &local_requests,
            max_requests: pool.options.max_requests,
            min_confidence: pool.options.min_confidence,
            refused: &refused,
            requests: pool.requests,
        };
        let Some(pair) = pack::prepare(&context, path)? else {
            dispatch::flush(pool, &mut pending, &mut assessed).await?;
            let result = assess::assess(&context, path).await?;
            assessed.work.over_budget += refused.get();
            assessed.results.push(result);
            continue;
        };
        if let Some(mut answers) = pool.review.answers.read(&pair.key) {
            if let (Some(applicability), Some(verification)) =
                (answers.remove("relevance"), answers.remove("violation"))
            {
                let mut result = pair.result;
                result.cached = true;
                result.shared_request = pool.review.answers.read_request_key(&pair.key);
                assess::apply_answers(
                    pool.options.min_confidence,
                    pool.emit,
                    applicability,
                    verification,
                    &mut result,
                )?;
                assessed.results.push(result);
                continue;
            }
            let mut result = pair.result;
            result.reason = Some("cached file assessment has an incomplete answer set".into());
            assessed.results.push(result);
            continue;
        }
        if !pending.can_push(
            pool.evaluator,
            pool.options.contract_batch_size,
            pool.evidence.plan.context_profile,
            &pair,
        )? {
            dispatch::flush(pool, &mut pending, &mut assessed).await?;
        }
        pending.push(pair);
    }
    dispatch::flush(pool, &mut pending, &mut assessed).await?;
    Ok(assessed)
}
