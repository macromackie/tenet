mod contracts;
mod render;
mod replay;

use alloc::collections::BTreeMap;
use core::cell::RefCell;
use std::{path::Path, time::Instant};

use ev_grep_core::RequestInfo;
use render::Report;
use tenet_engine::{
    Event,
    Plan,
    RunInfo,
    mechanical::{self, Evidence, Freshness, LocalMode, Outcome, ReceiptSet},
    review::{Checked, OverallStatus, summarize_overall},
};
use tokio::signal;

use super::{local::summarize_checks, prepare};
use crate::{
    cli::{Check, ExitStatus, ReporterKind},
    error::CommandError,
    generated,
};

/// Uses the ordinary semantic check and journal. Only this explicit mode runs project checkers.
pub(super) struct FinalReview<'run> {
    check: &'run Check,
    plan: &'run Plan,
    receipts: Option<ReceiptSet>,
    requests: RefCell<Vec<RequestInfo>>,
    run: RefCell<Option<RunInfo>>,
    started: Instant,
    state: &'run Path,
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Initialization, accounting, and finalization follow lifecycle order."
)]
impl<'run> FinalReview<'run> {
    pub(super) fn new(
        plan: &'run Plan,
        state: &'run Path,
        check: &'run Check,
        started: Instant,
    ) -> Result<Self, CommandError> {
        let receipts = check
            .evidence
            .as_ref()
            .map(|path| ReceiptSet::open(&plan.root, path))
            .transpose()?;
        Ok(Self {
            check,
            plan,
            receipts,
            requests: RefCell::new(Vec::new()),
            run: RefCell::new(None),
            started,
            state,
        })
    }

    /// Count each physical request at its owner. Cached and shared child results cost nothing new.
    pub(super) fn observe(&self, event: &Event) {
        let owner = match event {
            Event::FileBatchCompleted { request, .. }
            | Event::HunkResult {
                request,
                cached: false,
                ..
            } => request.as_ref(),
            Event::Result { result }
                if !result.cached && result.shared_request.is_none() && result.hunks.is_empty() =>
            {
                result.request.as_ref()
            },
            Event::ContractFinished {
                conclusion: Some(result),
                ..
            } if !result.cached => result.request.as_ref(),
            Event::Begin { run } => {
                drop(self.run.replace(Some(run.clone())));
                None
            },
            Event::CheckStarted { .. }
            | Event::ContractFinished { .. }
            | Event::ContractStarted { .. }
            | Event::Error { .. }
            | Event::FileBatchStarted { .. }
            | Event::HunkResult { .. }
            | Event::Result { .. }
            | Event::Skipped { .. }
            | Event::StageCompleted { .. }
            | Event::StageStarted { .. }
            | Event::Summary { .. } => None,
        };
        if let Some(info) = owner {
            self.requests.borrow_mut().push(info.clone());
        }
    }

    const fn is_json(&self) -> bool {
        self.check.run.json || matches!(self.check.run.reporter, ReporterKind::Jsonl)
    }

    #[expect(
        clippy::integer_division_remainder_used,
        reason = "The select macro uses a remainder for branch order."
    )]
    #[expect(
        clippy::future_not_send,
        reason = "The CLI runs this scoped review on its current thread."
    )]
    pub(super) async fn complete(
        &self,
        checked: &Checked,
        revision: Option<u64>,
    ) -> Result<ExitStatus, CommandError> {
        let mut errors = Vec::new();
        let observed = if self.check.evidence.is_none() {
            let mode = if self.check.fresh {
                LocalMode::Fresh
            } else {
                LocalMode::Reuse
            };
            tokio::select! {
                biased;
                signal = signal::ctrl_c() => {
                    signal?;
                    return self.report_stop(checked.summary, None);
                },
                rows = mechanical::run_selected_local(self.plan, self.state, None, mode) => {
                    rows.unwrap_or_else(|error| {errors.push(error.to_string()); Vec::new()})
                },
            }
        } else {
            Vec::new()
        };

        self.recheck(checked, revision, observed, errors).await
    }

    #[expect(
        clippy::future_not_send,
        reason = "Scoped CLI review uses single-threaded event accounting."
    )]
    async fn recheck(
        &self,
        checked: &Checked,
        revision: Option<u64>,
        observed: Vec<Evidence>,
        mut errors: Vec<String>,
    ) -> Result<ExitStatus, CommandError> {
        let generated = generated::assess(self.plan, self.state, self.check, checked).await?;
        // Rediscover as well as recapture. A checker or editor can add a new contract or file,
        // change an already checked input, or remove the original selection entirely.
        let refreshed = prepare(
            &self.plan.root,
            &self.check.input,
            self.plan.base.as_deref(),
            &self.plan.selected_paths,
        );
        let latest = refreshed.as_ref().ok().map(|(plan, ..)| plan);
        let capture = match latest {
            Some(plan) => Some(plan.capture().await),
            None => None,
        };
        let source_error = refreshed
            .as_ref()
            .err()
            .map(ToString::to_string)
            .or_else(|| capture.as_ref()?.as_ref().err().map(ToString::to_string));
        let packet = capture.as_ref().and_then(|result| result.as_ref().ok());
        let contracts = contracts::compare(
            self.plan,
            checked,
            latest.zip(packet),
            self.check,
            self.state,
        );
        let checks = self.refresh_checks(latest, observed, &mut errors);
        let status = summarize_final(
            checked,
            &contracts,
            &checks,
            source_error.is_some(),
            !errors.is_empty(),
        )
        .max(generated::summarize_status(&generated));
        let mut summary = checked.summary;
        summary.elapsed_ms = self.started.elapsed().as_millis();
        let report = Report {
            checks,
            contracts,
            errors,
            generated,
            revision,
            source_error,
            status,
            summary,
        };
        report.write(self)
    }

    fn refresh_checks(
        &self,
        latest: Option<&Plan>,
        observed: Vec<Evidence>,
        errors: &mut Vec<String>,
    ) -> Vec<Evidence> {
        let Some(plan) = latest else { return observed };
        self.verify_checks(plan, &observed).unwrap_or_else(|error| {
            errors.push(error.to_string());
            observed
        })
    }

    fn verify_checks(
        &self,
        plan: &Plan,
        observed: &[Evidence],
    ) -> Result<Vec<Evidence>, mechanical::ReceiptError> {
        if let Some(receipts) = &self.receipts {
            let mut rows = receipts.verify_plan(plan);
            for row in rows.iter_mut().filter(|row| {
                row.freshness != Freshness::Current || row.observed_outcome != Some(Outcome::Pass)
            }) {
                row.replay = Some(format!(
                    "Regenerate {}/{} on the trusted runner, then repeat the final check with \
                     --evidence.",
                    row.contract,
                    row.name.as_str()
                ));
            }
            return Ok(rows);
        }
        let mut verified = mechanical::inspect_selected_local(plan, self.state, None)?;
        let records: BTreeMap<_, _> = observed
            .iter()
            .map(|row| ((row.contract.as_str(), row.name.as_str()), row))
            .collect();
        for row in &mut verified {
            if let Some(prior) = records.get(&(row.contract.as_str(), row.name.as_str()))
                && prior.freshness == row.freshness
                && prior.observed_outcome == row.observed_outcome
            {
                row.reused = prior.reused;
                row.reason = row.reason.take().or_else(|| prior.reason.clone());
            }
        }
        Ok(verified)
    }
}

fn summarize_final(
    checked: &Checked,
    contracts: &[contracts::Row<'_>],
    checks: &[Evidence],
    source_unavailable: bool,
    check_error: bool,
) -> OverallStatus {
    if check_error {
        return OverallStatus::Error;
    }
    let mut status = checked.summarize_overall().max(summarize_overall(
        contracts.iter().map(contracts::Row::state),
    ));
    if source_unavailable || contracts.is_empty() {
        status = status.max(OverallStatus::Pending);
    }
    status.max(summarize_checks(checks))
}
