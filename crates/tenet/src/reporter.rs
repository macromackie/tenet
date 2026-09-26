use std::{
    collections::BTreeMap,
    io::{self, Write},
    time::Instant,
};

use anyhow::Result;
use console::Term;
use tenet_contracts::Contract;
use tenet_engine::{CheckResult, ContractStatus, Event, Stage, Status, Summary};

use crate::report_format::{contract_counts, counts, marker, paint, status, suite};
use crate::{cli::ReporterKind, diagnostics};

pub(crate) struct Reporter<'a> {
    kind: ReporterKind,
    contracts: &'a [Contract],
    evaluation: bool,
    term: Term,
    live: bool,
    running: bool,
    progress: crate::progress::Progress,
    started: Instant,
    contract_started: Instant,
    failures: Vec<diagnostics::Failure>,
    current: String,
    total: usize,
    done: usize,
    completing: bool,
    issues: Vec<CheckResult>,
    pub summary: Summary,
    contract_statuses: BTreeMap<&'static str, usize>,
}

impl<'a> Reporter<'a> {
    pub(crate) fn new(kind: ReporterKind, contracts: &'a [Contract], evaluation: bool) -> Self {
        let term = Term::buffered_stderr();
        let live = term.is_term()
            && std::env::var_os("CI").is_none()
            && std::env::var("TERM").as_deref() != Ok("dumb")
            && kind == ReporterKind::Default;
        Self {
            kind,
            contracts,
            evaluation,
            term,
            live,
            running: false,
            progress: crate::progress::Progress::default(),
            started: Instant::now(),
            contract_started: Instant::now(),
            failures: Vec::new(),
            current: String::new(),
            total: 0,
            done: 0,
            completing: false,
            issues: Vec::new(),
            summary: Summary::default(),
            contract_statuses: BTreeMap::new(),
        }
    }

    pub(crate) fn emit(&mut self, event: Event) -> Result<()> {
        self.track(&event);
        if self.kind == ReporterKind::Jsonl {
            let mut value = serde_json::to_value(event)?;
            value["version"] = 1.into();
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
                self.current = contract;
                self.contract_started = Instant::now();
                self.total = total;
                self.done = 0;
                self.completing = false;
                self.issues.clear();
            }
            Event::CheckStarted { .. } | Event::StageCompleted { .. } => {}
            Event::StageStarted { stage, .. } => {
                self.completing = stage == Stage::Completeness;
            }
            Event::Result { result } => {
                self.done += 1;
                let mismatch = result
                    .expected
                    .is_some_and(|expected| Status::from(expected) != result.status);
                if self.kind == ReporterKind::Verbose {
                    let label = result.example.as_deref().unwrap_or(&result.path);
                    let state = if self.evaluation {
                        if mismatch { "FAIL" } else { "PASS" }
                    } else {
                        status(result.status)
                    };
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
                if mismatch
                    || (!self.evaluation
                        && matches!(
                            result.status,
                            Status::Fail | Status::Uncertain | Status::Error | Status::Incomplete
                        ))
                {
                    self.issues.push(*result);
                }
            }
            Event::ContractFinished {
                contract,
                summary,
                conclusion,
            } => {
                let state = conclusion.as_ref().map_or_else(
                    || suite(&summary, self.evaluation),
                    |c| match c.status {
                        ContractStatus::Verified => "VERIFIED",
                        ContractStatus::Preserved => "PRESERVED",
                        ContractStatus::Unaffected => "UNAFFECTED",
                        ContractStatus::Failed => "FAILED",
                        ContractStatus::Unresolved => "UNRESOLVED",
                    },
                );
                *self.contract_statuses.entry(state).or_default() += 1;
                let text = format!(
                    "{} {}",
                    self.done,
                    if self.evaluation { "examples" } else { "files" }
                );
                self.term.write_line(&format!(
                    " {} {contract} ({text}) {}  {:.1}s",
                    marker(state),
                    paint(&state.to_lowercase()),
                    self.contract_started.elapsed().as_secs_f64()
                ))?;
                let failed = conclusion
                    .as_ref()
                    .is_some_and(|c| c.status == ContractStatus::Failed)
                    || (self.evaluation && summary.examples_failed > 0);
                if failed {
                    self.failures.push(diagnostics::Failure {
                        contract,
                        reason: conclusion.as_ref().map(|c| c.reason.clone()),
                        issues: std::mem::take(&mut self.issues),
                        evaluation: self.evaluation,
                    });
                } else if let Some(conclusion) = &conclusion
                    && conclusion.status == ContractStatus::Unresolved
                {
                    if let Some(error) = &conclusion.error {
                        self.term.write_line(&format!("  {error}"))?;
                    } else if self.kind == ReporterKind::Verbose {
                        self.term.write_line(&format!("  {}", conclusion.reason))?;
                    }
                }
                if let Some(issue) = self
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
                self.current.clear();
            }
            Event::Skipped { path, reason } => {
                self.term.write_line(&format!("SKIP {path}: {reason}"))?
            }
            Event::Summary { summary, .. } => {
                self.running = false;
                self.current.clear();
                for failure in &self.failures {
                    failure.write(self.contracts, &mut self.term)?;
                }
                self.term.write_line(&format!(
                    "\n Contracts  {}",
                    contract_counts(&self.contract_statuses, self.contracts.len())
                ))?;
                if self.evaluation || self.kind == ReporterKind::Verbose {
                    self.term.write_line(&format!(
                        "{}  {}",
                        if self.evaluation {
                            "Examples"
                        } else {
                            "File evidence"
                        },
                        counts(&summary, self.evaluation)
                    ))?;
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
            let phase = if self.completing {
                " · assessing contract"
            } else {
                ""
            };
            let active = format!(
                "{} {}/{} {}{phase}",
                self.current,
                self.done,
                self.total,
                if self.evaluation { "examples" } else { "files" }
            );
            self.progress.draw(
                &mut self.term,
                if self.current.is_empty() { "" } else { &active },
                &format!(
                    "Contracts  {}",
                    contract_counts(&self.contract_statuses, self.contracts.len())
                ),
                self.started.elapsed(),
            )?;
        }
        Ok(())
    }

    fn track(&mut self, event: &Event) {
        if matches!(event, Event::StageStarted { .. }) {
            self.summary.requests += 1;
        }
        if let Event::Result { result } = event {
            match result.status {
                Status::Pass => self.summary.passed += 1,
                Status::Fail => self.summary.failed += 1,
                Status::NotApplicable => self.summary.not_applicable += 1,
                Status::Uncertain => self.summary.uncertain += 1,
                Status::Error => self.summary.errors += 1,
                Status::Incomplete => self.summary.incomplete += 1,
            }
        }
    }
}

impl Drop for Reporter<'_> {
    fn drop(&mut self) {
        let _ = self.progress.clear(&self.term);
        let _ = self.term.flush();
    }
}
