mod state;
use state::ContractProgress;

use std::{
    collections::BTreeMap,
    io::{self, Write},
    time::Instant,
};

use anyhow::Result;
use console::Term;
use tenet_contracts::Contract;
use tenet_engine::{
    ContractState, ContractStatus, Event, Pending, Stage, Status, Summary, review::Answer,
};

use crate::report_format::{
    Model, contract_counts, counts, marker, paint, request_failure, status,
};
use crate::{cli::ReporterKind, diagnostics};

pub(crate) struct Reporter<'a> {
    kind: ReporterKind,
    contracts: &'a [Contract],
    /// The review state directory, named with the revision a check appends.
    state: &'a std::path::Path,
    term: Term,
    live: bool,
    running: bool,
    progress: crate::progress::Progress,
    started: Instant,
    failures: Vec<diagnostics::Failure>,
    active: BTreeMap<String, ContractProgress>,
    pub summary: Summary,
    contract_statuses: BTreeMap<&'static str, usize>,
    /// Contracts a record settled rather than the model.
    recorded: usize,
    /// Contract documents the diff deletes, listed with the summary.
    deleted: Vec<std::path::PathBuf>,
    diff: bool,
    /// The run's confidence threshold, shown beside an unresolved answer that fell short of it.
    min_confidence: f64,
}

impl<'a> Reporter<'a> {
    pub(crate) fn new(
        kind: ReporterKind,
        contracts: &'a [Contract],
        state: &'a std::path::Path,
    ) -> Self {
        let term = Term::buffered_stderr();
        let live = term.is_term()
            && std::env::var_os("CI").is_none()
            && std::env::var("TERM").as_deref() != Ok("dumb")
            && kind == ReporterKind::Default;
        Self {
            kind,
            contracts,
            state,
            term,
            live,
            running: false,
            progress: crate::progress::Progress::default(),
            started: Instant::now(),
            failures: Vec::new(),
            active: BTreeMap::new(),
            summary: Summary::default(),
            contract_statuses: BTreeMap::new(),
            recorded: 0,
            deleted: Vec::new(),
            diff: false,
            min_confidence: tenet_engine::MIN_CONFIDENCE,
        }
    }

    pub(crate) fn emit(&mut self, event: Event) -> Result<()> {
        self.track(&event);
        if self.kind == ReporterKind::Jsonl {
            let failed = match &event {
                Event::Result { result } => result.failure.map(|failure| {
                    (
                        &result.contract,
                        result.path.as_str(),
                        failure,
                        &result.reason,
                    )
                }),
                Event::ContractFinished {
                    contract,
                    conclusion: Some(conclusion),
                    ..
                } => conclusion
                    .failure
                    .map(|failure| (contract, "combined assessment", failure, &conclusion.error)),
                _ => None,
            }
            .map(|(contract, subject, request, message)| {
                format!(
                    "error: {contract} > {subject}: request failed ({}): {}",
                    request_failure(request),
                    message.as_deref().unwrap_or_default()
                )
            });
            let mut value = serde_json::to_value(event)?;
            value["version"] = 4.into();
            let mut out = io::stdout().lock();
            serde_json::to_writer(&mut out, &value)?;
            writeln!(out)?;
            out.flush()?;
            // An agent that reads only stderr still learns which requests failed.
            if let Some(line) = failed {
                writeln!(io::stderr(), "{line}")?;
            }
            return Ok(());
        }
        let redraw = matches!(
            event,
            Event::ContractStarted { .. }
                | Event::ContractFinished { .. }
                | Event::Skipped { .. }
                | Event::Error { .. }
        );
        if matches!(
            event,
            Event::Begin { .. }
                | Event::ContractFinished { .. }
                | Event::Skipped { .. }
                | Event::Summary { .. }
                | Event::Error { .. }
        ) {
            self.progress.clear(&self.term)?;
        }
        match event {
            Event::Begin { run } => {
                self.started = Instant::now();
                self.term
                    .write_line(&format!("TENET {}  {}\n", run.version, run.root))?;
                self.term.write_line(&format!("Mode: {}", run.mode))?;
                self.diff = run.base.is_some();
                self.deleted = run.deleted_contracts;
                self.min_confidence = run.min_confidence;
                if let Some(assumption) = run.assumption {
                    self.term.write_line(&format!("Assumption: {assumption}"))?;
                }
                if run.model.is_none() {
                    self.term
                        .write_line("Model: none; unchanged inputs reuse earlier conclusions")?;
                }
            }
            Event::ContractStarted {
                contract, total, ..
            } => {
                self.running = true;
                self.active.insert(
                    contract,
                    ContractProgress {
                        total,
                        ..Default::default()
                    },
                );
            }
            Event::CheckStarted { .. } | Event::StageCompleted { .. } => {}
            Event::StageStarted {
                contract, stage, ..
            } => {
                if let Some(progress) = self.active.get_mut(&contract) {
                    progress.completing = stage == Stage::Completeness;
                }
            }
            Event::Result { result } => {
                let progress = self.active.entry(result.contract.clone()).or_default();
                progress.done += 1;
                if self.kind == ReporterKind::Verbose {
                    let label = &result.path;
                    let state = status(result.status);
                    self.term.write_line(&format!(
                        "  {} {} > {label} ({}ms)",
                        paint(state),
                        result.contract,
                        result.elapsed_ms
                    ))?;
                    if let Some(reason) = &result.reason {
                        self.term.write_line(&format!("    {reason}"))?;
                    }
                }
                if matches!(
                    result.status,
                    Status::Fail | Status::Uncertain | Status::Error | Status::Incomplete
                ) {
                    progress.issues.push(*result);
                }
            }
            Event::ContractFinished {
                contract,
                summary,
                conclusion,
                state,
            } => {
                let mut progress = self.active.remove(&contract).unwrap_or_default();
                let model = conclusion.as_deref().map(|c| Model {
                    status: c.status,
                    violation: c.violation,
                    answer: c.assessment.as_ref().map(|answer| Answer {
                        choice: answer.choice,
                        confidence: answer.confidence,
                    }),
                    min_confidence: Some(self.min_confidence),
                });
                let unchanged = if conclusion.as_ref().is_some_and(|c| c.cached) {
                    " · unchanged"
                } else {
                    ""
                };
                let (label, note) = crate::report_format::state(&state, model, unchanged);
                *self.contract_statuses.entry(label).or_default() += 1;
                self.recorded += usize::from(matches!(
                    state,
                    ContractState::Settled {
                        record: Some(_),
                        ..
                    }
                ));
                let files = if progress.done > 0 {
                    progress.done
                } else {
                    progress.total
                };
                let text = format!("{files} file{}", if files == 1 { "" } else { "s" });
                self.term.write_line(&format!(
                    " {} {contract} ({text}) {} · {note}  {:.1}s",
                    marker(label),
                    paint(&label.to_lowercase()),
                    summary.elapsed_ms as f64 / 1000.0
                ))?;
                let failed = matches!(
                    state,
                    ContractState::Pending {
                        reason: Pending::ModelFailed
                    } | ContractState::Settled {
                        conclusion: ContractStatus::Failed,
                        ..
                    }
                );
                let record = match &state {
                    ContractState::Settled {
                        record: Some(id), ..
                    } => Some(id),
                    _ => None,
                };
                if let Some(id) = record {
                    if failed {
                        self.failures.push(diagnostics::Failure {
                            reason: Some(format!(
                                "Recorded {id}; tenet show {contract} gives its evidence."
                            )),
                            contract,
                            issues: Vec::new(),
                        });
                    }
                } else if failed {
                    self.failures.push(diagnostics::Failure {
                        contract,
                        reason: conclusion.as_ref().map(|c| c.reason.clone()),
                        issues: std::mem::take(&mut progress.issues),
                    });
                } else if let Some(conclusion) = &conclusion
                    && conclusion.status == ContractStatus::Unresolved
                {
                    if !conclusion.conflicting_files.is_empty() {
                        self.term.write_line(&format!(
                            "  Assessments disagree: {}",
                            conclusion.conflicting_files.join(", ")
                        ))?;
                    }
                    if let Some(error) = &conclusion.error {
                        let kind = conclusion
                            .failure
                            .map(|request| format!(" ({})", request_failure(request)))
                            .unwrap_or_default();
                        self.term.write_line(&format!("  {error}{kind}"))?;
                    } else {
                        self.term.write_line(&format!("  {}", conclusion.reason))?;
                    }
                } else if let Some(conclusion) = &conclusion
                    && conclusion.status == ContractStatus::Clear
                {
                    self.term.write_line(&format!("  {}", conclusion.reason))?;
                }
                if let Some(context) = conclusion.as_ref().and_then(|c| c.context.as_ref())
                    && context.is_reduced()
                {
                    self.term.write_line(&format!(
                        "  Combined input {}",
                        crate::report_format::context(context)
                    ))?;
                }
                let mut issues = progress
                    .issues
                    .iter()
                    .filter(|r| matches!(r.status, Status::Error | Status::Incomplete));
                if let Some(issue) = issues.next() {
                    let more = match issues.count() {
                        0 => String::new(),
                        more => format!("; {more} more could not complete"),
                    };
                    self.term.write_line(&format!(
                        "  {}: {}{}{more}",
                        issue.path,
                        issue
                            .reason
                            .as_deref()
                            .unwrap_or("check could not complete"),
                        issue
                            .failure
                            .map(|request| format!(" ({})", request_failure(request)))
                            .unwrap_or_default()
                    ))?;
                }
            }
            Event::Skipped { path, reason } => {
                self.term.write_line(&format!("SKIP {path}: {reason}"))?
            }
            Event::Summary {
                summary,
                revision,
                pending,
                coverage,
                ..
            } => {
                self.running = false;

                for failure in &self.failures {
                    failure.write(self.contracts, &mut self.term)?;
                }
                let recorded = if self.recorded > 0 {
                    format!(" · {} settled by record", self.recorded)
                } else {
                    String::new()
                };
                self.term.write_line(&format!(
                    "\n Contracts  {}{recorded}",
                    contract_counts(&self.contract_statuses, self.contracts.len())
                ))?;
                match coverage.as_ref().and_then(crate::report_format::coverage) {
                    Some(coverage) => self.term.write_line(&format!(" {coverage}"))?,
                    None if self.diff && self.contracts.is_empty() => self
                        .term
                        .write_line(" No contract's scope has changes since the base.")?,
                    None => {}
                }
                for path in &self.deleted {
                    self.term.write_line(&format!(
                        " Deleted    {}; its requirement no longer applies",
                        path.display()
                    ))?;
                }
                if self.kind == ReporterKind::Verbose {
                    self.term
                        .write_line(&format!("File evidence  {}", counts(&summary)))?;
                }
                let cached = if summary.cached > 0 {
                    format!(" ({} answered from earlier checks)", summary.cached)
                } else {
                    String::new()
                };
                let over_budget = if summary.over_budget > 0 {
                    format!(
                        " · budget spent; {} more not sent, so their checks are incomplete",
                        summary.over_budget
                    )
                } else {
                    String::new()
                };
                // Only a check that keeps a revision keeps answers for a rerun to reuse.
                let failed = match (summary.request_failures, summary.retryable_failures) {
                    (0, _) => String::new(),
                    (failures, retryable) if retryable > 0 && revision.is_some() => format!(
                        " · {failures} failed ({retryable} retryable); a rerun sends only failed requests"
                    ),
                    (failures, retryable) => {
                        format!(" · {failures} failed ({retryable} retryable)")
                    }
                };
                self.term.write_line(&format!(
                    " Requests   {}{cached}{over_budget}{failed}\n Duration   {:.2}s",
                    summary.requests,
                    summary.elapsed_ms as f64 / 1000.0
                ))?;
                if let Some(revision) = revision {
                    self.term.write_line(&format!(
                        " Revision   r{revision} in {}",
                        self.state.join("events.jsonl").display()
                    ))?;
                }
                if summary.cancelled {
                    self.term
                        .write_line("Cancelled; remaining checks were not completed.")?;
                } else if let Some(first) = pending.first() {
                    let shown = pending
                        .iter()
                        .take(5)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ");
                    let more = pending.len().saturating_sub(5);
                    self.term.write_line(&format!(
                        " Pending    {} · {shown}{}",
                        pending.len(),
                        if more > 0 {
                            format!(" and {more} more")
                        } else {
                            String::new()
                        }
                    ))?;
                    let next = if revision.is_some() {
                        format!("tenet show {first}")
                    } else {
                        "rerun without --snapshot to record conclusions".into()
                    };
                    self.term.write_line(&format!(" Next       {next}"))?;
                }
            }
            Event::Error { message } => self.term.write_line(&format!("ERROR {message}"))?,
        }
        if redraw {
            self.refresh()?;
        }
        self.term.flush()?;
        Ok(())
    }

    pub(crate) fn is_live(&self) -> bool {
        self.live
    }

    pub(crate) fn refresh(&mut self) -> Result<()> {
        if self.live && self.running {
            let current = self.contracts.iter().find_map(|contract| {
                self.active
                    .get(&contract.name)
                    .map(|progress| (&contract.name, progress))
            });
            let active = current
                .map(|(name, progress)| {
                    let phase = if progress.completing {
                        " · assessing contract"
                    } else {
                        ""
                    };
                    let others = self.active.len().saturating_sub(1);
                    let extra = if others > 0 {
                        format!(" · {others} other contracts")
                    } else {
                        String::new()
                    };
                    format!(
                        "{name} {}/{} files{phase}{extra}",
                        progress.done, progress.total
                    )
                })
                .unwrap_or_default();
            self.progress.draw(
                &mut self.term,
                &active,
                &format!(
                    "Contracts  {}",
                    contract_counts(&self.contract_statuses, self.contracts.len())
                ),
                self.started.elapsed(),
            )?;
        }
        Ok(())
    }
}

impl Drop for Reporter<'_> {
    fn drop(&mut self) {
        let _ = self.progress.clear(&self.term);
        let _ = self.term.flush();
    }
}
