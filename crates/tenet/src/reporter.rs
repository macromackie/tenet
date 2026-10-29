mod state;
use alloc::collections::BTreeMap;
use core::mem;
use std::{
    env,
    io::{self, Write as _},
    path::{Path, PathBuf},
    time::Instant,
};

use console::Term;
use state::ContractProgress;
use tenet_contracts::Contract;
use tenet_engine::{
    ContractState,
    ContractStatus,
    Event,
    Pending,
    Stage,
    Status,
    Summary,
    review::Answer,
};

use crate::{
    cli::ReporterKind,
    diagnostics,
    error::CommandError,
    output,
    progress::Progress,
    report_format,
    report_format::{
        Model,
        format_contract_counts,
        format_counts,
        format_marker,
        format_painted,
        label_status,
        request_failure,
    },
    telemetry,
};

/// The results a contract's progress lists as issues.
const ISSUE_STATUSES: [Status; 4] = [
    Status::Fail,
    Status::Uncertain,
    Status::Error,
    Status::Incomplete,
];
/// The results that left a check unfinished.
const UNFINISHED_STATUSES: [Status; 2] = [Status::Error, Status::Incomplete];

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Fields and methods are grouped by the stage of a check that uses them."
)]
#[expect(
    clippy::partial_pub_fields,
    reason = "The private fields hold state that only this module may set."
)]
pub(crate) struct Reporter<'check> {
    kind: ReporterKind,
    contracts: &'check [Contract],
    /// The review state directory, named with the revision a check appends.
    state: &'check Path,
    term: Term,
    live: bool,
    running: bool,
    progress: Progress,
    started: Instant,
    failures: Vec<diagnostics::Failure>,
    active_contracts: BTreeMap<String, ContractProgress>,
    pub summary: Summary,
    contract_statuses: BTreeMap<&'static str, usize>,
    /// Contracts a record settled rather than the model.
    recorded: usize,
    /// Contract documents the diff deletes, listed with the summary.
    deleted: Vec<PathBuf>,
    diff: bool,
    /// The run's confidence threshold, shown beside an unresolved answer that fell short of it.
    min_confidence: f64,
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Fields and methods are grouped by the stage of a check that uses them."
)]
impl<'check> Reporter<'check> {
    pub(crate) fn new(
        kind: ReporterKind,
        contracts: &'check [Contract],
        state: &'check Path,
    ) -> Self {
        let term = output::open_terminal();
        let live = term.is_term()
            && env::var_os("CI").is_none()
            && env::var("TERM").as_deref() != Ok("dumb")
            && kind == ReporterKind::Default;
        Self {
            kind,
            contracts,
            state,
            term,
            live,
            running: false,
            progress: Progress::default(),
            started: Instant::now(),
            failures: Vec::new(),
            active_contracts: BTreeMap::new(),
            summary: Summary::default(),
            contract_statuses: BTreeMap::new(),
            recorded: 0,
            deleted: Vec::new(),
            diff: false,
            min_confidence: tenet_engine::MIN_CONFIDENCE,
        }
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Counters are bounded by the requests and contracts of one run."
    )]
    #[expect(
        clippy::as_conversions,
        reason = "The converted values are request budgets, byte caps, durations and fixture \
                  positions that fit the target type."
    )]
    #[expect(
        clippy::cast_precision_loss,
        reason = "Elapsed milliseconds stay far below 2^52, and the seconds are shown rounded."
    )]
    #[expect(
        clippy::cognitive_complexity,
        reason = "The steps run in one order and share local state; splitting them would add \
                  forwarding functions."
    )]
    #[expect(
        clippy::else_if_without_else,
        reason = "`needless_else` rejects the empty `else` this lint asks for."
    )]
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    #[expect(
        clippy::float_arithmetic,
        reason = "The values are display text: elapsed seconds and violation percentages."
    )]
    #[expect(
        clippy::shadow_reuse,
        reason = "Each step rebinds the value it converts under the same name."
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "The steps run in one order and share local state; splitting them would add \
                  forwarding functions."
    )]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The remaining variants take no action here, and listing each would repeat the \
                  enum."
    )]
    pub(crate) fn emit(&mut self, event: Event) -> Result<(), CommandError> {
        self.track(&event);
        let unanswered = match &event {
            Event::Result { result } => {
                result.failure.map(|failure| {
                    (
                        &result.contract,
                        result.path.as_str(),
                        failure,
                        &result.reason,
                    )
                })
            },
            Event::ContractFinished {
                contract,
                conclusion: Some(conclusion),
                ..
            } => {
                conclusion
                    .failure
                    .map(|failure| (contract, "combined assessment", failure, &conclusion.error))
            },
            _ => None,
        };
        if let Some((_, _, failure, _)) = unanswered {
            telemetry::record_request_failed(failure);
        }
        if self.kind == ReporterKind::Jsonl {
            let failed = unanswered.map(|(contract, subject, request, message)| {
                format!(
                    "error: {contract} > {subject}: request failed ({}): {}",
                    request_failure(request),
                    message.as_deref().unwrap_or_default()
                )
            });
            let mut out = output::open_stdout().lock();
            output::write_document(
                &mut out,
                tenet_engine::EVENTS_VERSION,
                serde_json::to_value(event)?,
            )?;
            out.flush()?;
            // An agent that reads only stderr still learns which requests failed.
            if let Some(line) = failed {
                writeln!(output::open_stderr(), "{line}")?;
            }
            return Ok(());
        }
        let redraw = match event {
            Event::ContractStarted { .. }
            | Event::ContractFinished { .. }
            | Event::Skipped { .. }
            | Event::Error { .. } => true,
            Event::Begin { .. }
            | Event::CheckStarted { .. }
            | Event::Result { .. }
            | Event::StageCompleted { .. }
            | Event::StageStarted { .. }
            | Event::Summary { .. } => false,
        };
        let clears = match event {
            Event::Begin { .. }
            | Event::ContractFinished { .. }
            | Event::Skipped { .. }
            | Event::Summary { .. }
            | Event::Error { .. } => true,
            Event::CheckStarted { .. }
            | Event::ContractStarted { .. }
            | Event::Result { .. }
            | Event::StageCompleted { .. }
            | Event::StageStarted { .. } => false,
        };
        if clears {
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
            },
            Event::ContractStarted {
                contract, total, ..
            } => {
                self.running = true;
                drop(self.active_contracts.insert(
                    contract,
                    ContractProgress {
                        total,
                        ..Default::default()
                    },
                ));
            },
            Event::CheckStarted { .. } | Event::StageCompleted { .. } => {},
            Event::StageStarted {
                contract, stage, ..
            } => {
                if let Some(progress) = self.active_contracts.get_mut(&contract) {
                    progress.completing = stage == Stage::Completeness;
                }
            },
            Event::Result { result } => {
                let progress = self
                    .active_contracts
                    .entry(result.contract.clone())
                    .or_default();
                progress.done += 1;
                if self.kind == ReporterKind::Verbose {
                    let label = &result.path;
                    let state = label_status(result.status);
                    self.term.write_line(&format!(
                        "  {} {} > {label} ({}ms)",
                        format_painted(state),
                        result.contract,
                        result.elapsed_ms
                    ))?;
                    if let Some(reason) = &result.reason {
                        self.term.write_line(&format!("    {reason}"))?;
                    }
                }
                if ISSUE_STATUSES.contains(&result.status) {
                    progress.issues.push(*result);
                }
            },
            Event::ContractFinished {
                contract,
                summary,
                conclusion,
                state,
            } => {
                let mut progress = self.active_contracts.remove(&contract).unwrap_or_default();
                let model = conclusion.as_deref().map(|verdict| {
                    Model {
                        status: verdict.status,
                        violation: verdict.violation,
                        answer: verdict.assessment.as_ref().map(|answer| {
                            Answer {
                                choice: answer.choice,
                                confidence: answer.confidence,
                            }
                        }),
                        min_confidence: Some(self.min_confidence),
                    }
                });
                let unchanged = if conclusion.as_ref().is_some_and(|verdict| verdict.cached) {
                    " \u{b7} unchanged"
                } else {
                    ""
                };
                let (label, note) = report_format::format_state(&state, model, unchanged);
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
                    format_marker(label),
                    format_painted(&label.to_lowercase()),
                    summary.elapsed_ms as f64 / 1_000.0_f64
                ))?;
                let failed = match state {
                    ContractState::Pending { reason } => reason == Pending::ModelFailed,
                    ContractState::Settled {
                        conclusion: settled,
                        ..
                    } => settled == ContractStatus::Failed,
                };
                let record = match &state {
                    ContractState::Settled {
                        record: Some(id), ..
                    } => Some(id),
                    _ => None,
                };
                if let Some(id) = record {
                    if failed {
                        let reason =
                            format!("Recorded {id}; tenet show {contract} gives its evidence.");
                        self.failures.push(diagnostics::Failure {
                            contract,
                            issues: Vec::new(),
                            reason: Some(reason),
                        });
                    }
                } else if failed {
                    self.failures.push(diagnostics::Failure {
                        contract,
                        issues: mem::take(&mut progress.issues),
                        reason: conclusion.as_ref().map(|verdict| verdict.reason.clone()),
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
                if let Some(context) = conclusion
                    .as_ref()
                    .and_then(|verdict| verdict.context.as_ref())
                    && context.is_reduced()
                {
                    self.term.write_line(&format!(
                        "  Combined input {}",
                        report_format::format_context(context)
                    ))?;
                }
                let mut issues = progress
                    .issues
                    .iter()
                    .filter(|issue| UNFINISHED_STATUSES.contains(&issue.status));
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
            },
            Event::Skipped { path, reason } => {
                self.term.write_line(&format!("SKIP {path}: {reason}"))?;
            },
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
                    format_contract_counts(&self.contract_statuses, self.contracts.len())
                ))?;
                match coverage.as_ref().and_then(report_format::format_coverage) {
                    Some(coverage) => self.term.write_line(&format!(" {coverage}"))?,
                    None if self.diff && self.contracts.is_empty() => {
                        self.term
                            .write_line(" No contract's scope has changes since the base.")?;
                    },
                    None => {},
                }
                for path in &self.deleted {
                    self.term.write_line(&format!(
                        " Deleted    {}; its requirement no longer applies",
                        path.display()
                    ))?;
                }
                if self.kind == ReporterKind::Verbose {
                    self.term
                        .write_line(&format!("File evidence  {}", format_counts(&summary)))?;
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
                    (failures, retryable) if retryable > 0 && revision.is_some() => {
                        format!(
                            " · {failures} failed ({retryable} retryable); a rerun sends only \
                             failed requests"
                        )
                    },
                    (failures, retryable) => {
                        format!(" · {failures} failed ({retryable} retryable)")
                    },
                };
                self.term.write_line(&format!(
                    " Requests   {}{cached}{over_budget}{failed}\n Duration   {:.2}s",
                    summary.requests,
                    summary.elapsed_ms as f64 / 1_000.0_f64
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
            },
            Event::Error { message } => self.term.write_line(&format!("ERROR {message}"))?,
        }
        if redraw {
            self.refresh()?;
        }
        self.term.flush()?;
        Ok(())
    }

    pub(crate) const fn is_live(&self) -> bool {
        self.live
    }

    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    pub(crate) fn refresh(&mut self) -> io::Result<()> {
        if self.live && self.running {
            let current = self.contracts.iter().find_map(|contract| {
                self.active_contracts
                    .get(&contract.name)
                    .map(|progress| (&contract.name, progress))
            });
            let active = current
                .map(|(name, progress)| {
                    let phase = if progress.completing {
                        " \u{b7} assessing contract"
                    } else {
                        ""
                    };
                    let others = self.active_contracts.len().saturating_sub(1);
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
                &self.term,
                &active,
                &format!(
                    "Contracts  {}",
                    format_contract_counts(&self.contract_statuses, self.contracts.len())
                ),
                self.started.elapsed(),
            )?;
        }
        Ok(())
    }
}

impl Drop for Reporter<'_> {
    fn drop(&mut self) {
        // A write that fails on the way out has nowhere left to be reported.
        drop(self.progress.clear(&self.term));
        drop(self.term.flush());
    }
}
