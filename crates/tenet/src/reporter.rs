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
use tenet_engine::{ContractStatus, Event, Stage, Status, Summary};

use crate::report_format::{contract_counts, counts, marker, paint, status, suite};
use crate::{cli::ReporterKind, diagnostics};

pub(crate) struct Reporter<'a> {
    kind: ReporterKind,
    contracts: &'a [Contract],
    term: Term,
    live: bool,
    running: bool,
    progress: crate::progress::Progress,
    started: Instant,
    failures: Vec<diagnostics::Failure>,
    active: BTreeMap<String, ContractProgress>,
    pub summary: Summary,
    contract_statuses: BTreeMap<&'static str, usize>,
}

impl<'a> Reporter<'a> {
    pub(crate) fn new(kind: ReporterKind, contracts: &'a [Contract]) -> Self {
        let term = Term::buffered_stderr();
        let live = term.is_term()
            && std::env::var_os("CI").is_none()
            && std::env::var("TERM").as_deref() != Ok("dumb")
            && kind == ReporterKind::Default;
        Self {
            kind,
            contracts,
            term,
            live,
            running: false,
            progress: crate::progress::Progress::default(),
            started: Instant::now(),
            failures: Vec::new(),
            active: BTreeMap::new(),
            summary: Summary::default(),
            contract_statuses: BTreeMap::new(),
        }
    }

    pub(crate) fn emit(&mut self, event: Event) -> Result<()> {
        self.track(&event);
        if self.kind == ReporterKind::Jsonl {
            let mut value = serde_json::to_value(event)?;
            value["version"] = 2.into();
            let mut out = io::stdout().lock();
            serde_json::to_writer(&mut out, &value)?;
            writeln!(out)?;
            out.flush()?;
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
                if let Some(assumption) = run.assumption {
                    self.term.write_line(&format!("Assumption: {assumption}"))?;
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
            } => {
                let mut progress = self.active.remove(&contract).unwrap_or_default();
                let state = conclusion.as_ref().map_or_else(
                    || suite(&summary),
                    |c| match c.status {
                        ContractStatus::Verified => "VERIFIED",
                        ContractStatus::Preserved => "PRESERVED",
                        ContractStatus::Unaffected => "UNAFFECTED",
                        ContractStatus::Failed => "FAILED",
                        ContractStatus::Unresolved => "UNRESOLVED",
                    },
                );
                *self.contract_statuses.entry(state).or_default() += 1;
                let text = format!("{} files", progress.done);
                self.term.write_line(&format!(
                    " {} {contract} ({text}) {}{}  {:.1}s",
                    marker(state),
                    paint(&state.to_lowercase()),
                    conclusion
                        .as_ref()
                        .and_then(|c| c.confidence)
                        .map(|score| format!(" ({:.0}% confidence)", score * 100.0))
                        .unwrap_or_default(),
                    summary.elapsed_ms as f64 / 1000.0
                ))?;
                let failed = conclusion
                    .as_ref()
                    .is_some_and(|c| c.status == ContractStatus::Failed);
                if failed {
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
                        self.term.write_line(&format!("  {error}"))?;
                    } else if self.kind == ReporterKind::Verbose {
                        self.term.write_line(&format!("  {}", conclusion.reason))?;
                    }
                }
                if let Some(issue) = progress
                    .issues
                    .iter()
                    .find(|r| matches!(r.status, Status::Error | Status::Incomplete))
                {
                    self.term.write_line(&format!(
                        "  {}: {}",
                        issue.path,
                        issue
                            .reason
                            .as_deref()
                            .unwrap_or("check could not complete")
                    ))?;
                }
            }
            Event::Skipped { path, reason } => {
                self.term.write_line(&format!("SKIP {path}: {reason}"))?
            }
            Event::Summary { summary, .. } => {
                self.running = false;

                for failure in &self.failures {
                    failure.write(self.contracts, &mut self.term)?;
                }
                self.term.write_line(&format!(
                    "\n Contracts  {}",
                    contract_counts(&self.contract_statuses, self.contracts.len())
                ))?;
                if self.kind == ReporterKind::Verbose {
                    self.term
                        .write_line(&format!("File evidence  {}", counts(&summary)))?;
                }
                self.term.write_line(&format!(
                    " Requests   {}\n Duration   {:.2}s",
                    summary.requests,
                    summary.elapsed_ms as f64 / 1000.0
                ))?;
                if summary.cancelled {
                    self.term
                        .write_line("Cancelled; remaining checks were not completed.")?;
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
