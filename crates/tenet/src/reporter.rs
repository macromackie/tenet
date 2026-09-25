use std::{
    collections::BTreeMap,
    io::{self, Write},
};

use anyhow::Result;
use console::Term;
use tenet_contracts::Contract;
use tenet_engine::{Event, Stage, Status, Summary};

use crate::{cli::ReporterKind, diagnostics};

pub(crate) struct Reporter<'a> {
    kind: ReporterKind,
    contracts: &'a [Contract],
    evaluation: bool,
    term: Term,
    live: bool,
    drawn: bool,
    current: String,
    total: usize,
    done: usize,
    active: usize,
    relevance: usize,
    verification: usize,
    pub summary: Summary,
    contract_statuses: BTreeMap<&'static str, usize>,
}

impl<'a> Reporter<'a> {
    pub(crate) fn new(kind: ReporterKind, contracts: &'a [Contract], evaluation: bool) -> Self {
        let term = Term::stderr();
        let live =
            term.is_term() && std::env::var_os("CI").is_none() && kind == ReporterKind::Default;
        Self {
            kind,
            contracts,
            evaluation,
            term,
            live,
            drawn: false,
            current: String::new(),
            total: 0,
            done: 0,
            active: 0,
            relevance: 0,
            verification: 0,
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
        self.clear()?;
        match event {
            Event::Begin { run } => {
                self.term
                    .write_line(&format!("TENET {}  {}\n", run.version, run.root))?;
                if run.broadened {
                    self.term.write_line(
                        "Contract or ignore changes: checking the full selected scope.\n",
                    )?;
                }
            }
            Event::ContractStarted {
                contract, total, ..
            } => {
                self.current = contract;
                self.total = total;
                self.done = 0;
                self.active = 0;
                self.relevance = 0;
                self.verification = 0;
            }
            Event::CheckStarted { .. } => self.active += 1,
            Event::StageStarted { .. } => {}
            Event::StageCompleted { stage, .. } => match stage {
                Stage::Applicability => self.relevance += 1,
                Stage::Verification => self.verification += 1,
            },
            Event::Result { result } => {
                self.active = self.active.saturating_sub(1);
                self.done += 1;
                let mismatch = result
                    .expected
                    .is_some_and(|expected| Status::from(expected) != result.status);
                let problem = mismatch
                    || (!self.evaluation
                        && matches!(
                            result.status,
                            Status::Fail | Status::Uncertain | Status::Error | Status::Incomplete
                        ));
                if problem || self.kind == ReporterKind::Verbose {
                    let label = result.example.as_deref().unwrap_or(&result.path);
                    let status = if self.evaluation {
                        if mismatch { "FAIL" } else { "PASS" }
                    } else {
                        status(result.status)
                    };
                    self.term.write_line(&format!(
                        "  {} {} > {label} ({}ms)",
                        paint(status),
                        result.contract,
                        result.elapsed_ms
                    ))?;
                    if let Some(expected) = result.expected {
                        self.term.write_line(&format!(
                            "    expected {} · received {}",
                            status_name(Status::from(expected)),
                            status_name(result.status)
                        ))?;
                    }
                    if problem
                        && let Some(contract) =
                            self.contracts.iter().find(|c| c.name == result.contract)
                    {
                        diagnostics::finding(contract, &result, &mut self.term)?;
                    }
                }
            }
            Event::ContractFinished { contract, summary } => {
                let state = suite(&summary, self.evaluation);
                *self.contract_statuses.entry(state).or_default() += 1;
                let text = counts(&summary, self.evaluation);
                self.term
                    .write_line(&format!("{} {contract}  {text}\n", paint(state)))?;
                self.current.clear();
            }
            Event::Skipped { path, reason } => {
                self.term.write_line(&format!("SKIP {path}: {reason}"))?
            }
            Event::Summary { summary, exit_code } => {
                self.current.clear();
                self.term.write_line(&format!(
                    "Contracts  {}",
                    self.contract_statuses
                        .iter()
                        .map(|(k, v)| format!("{v} {}", k.to_lowercase()))
                        .collect::<Vec<_>>()
                        .join(" · ")
                ))?;
                self.term.write_line(&format!(
                    "{}  {}",
                    if self.evaluation {
                        "Examples"
                    } else {
                        "Checks"
                    },
                    counts(&summary, self.evaluation)
                ))?;
                self.term.write_line(&format!(
                    "Requests   {}\nDuration   {:.2}s\nExit       {exit_code}",
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
        if self.live && !self.current.is_empty() {
            self.term.write_line(&format!(
                "❯ {}  {}/{} complete · {} running · relevance {} · verification {}",
                self.current, self.done, self.total, self.active, self.relevance, self.verification
            ))?;
            self.drawn = true;
        }
        Ok(())
    }

    fn clear(&mut self) -> Result<()> {
        if self.drawn {
            self.term.clear_last_lines(1)?;
            self.drawn = false;
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
        let _ = self.clear();
    }
}

fn status(state: Status) -> &'static str {
    match state {
        Status::Pass => "PASS",
        Status::Fail => "FAIL",
        Status::NotApplicable => "EXCLUDED",
        Status::Uncertain => "UNCERTAIN",
        Status::Error => "ERROR",
        Status::Incomplete => "INCOMPLETE",
    }
}
fn status_name(state: Status) -> &'static str {
    match state {
        Status::Pass => "pass",
        Status::Fail => "fail",
        Status::NotApplicable => "not_applicable",
        Status::Uncertain => "uncertain",
        Status::Error => "error",
        Status::Incomplete => "incomplete",
    }
}
fn suite(s: &Summary, evaluation: bool) -> &'static str {
    if s.errors > 0 {
        "ERROR"
    } else if s.incomplete > 0 {
        "INCOMPLETE"
    } else if evaluation {
        if s.examples_failed > 0 {
            "FAIL"
        } else if s.examples_passed > 0 {
            "PASS"
        } else {
            "EMPTY"
        }
    } else if s.failed > 0 {
        "FAIL"
    } else if s.uncertain > 0 {
        "UNCERTAIN"
    } else if s.passed > 0 {
        "PASS"
    } else {
        "EMPTY"
    }
}
fn counts(s: &Summary, evaluation: bool) -> String {
    if evaluation {
        format!(
            "{} passed · {} failed · {} errors · {} incomplete",
            s.examples_passed, s.examples_failed, s.errors, s.incomplete
        )
    } else {
        format!(
            "{} passed · {} failed · {} uncertain · {} not applicable · {} errors · {} incomplete",
            s.passed, s.failed, s.uncertain, s.not_applicable, s.errors, s.incomplete
        )
    }
}

fn paint(state: &str) -> String {
    let styled = console::style(state).for_stderr();
    match state {
        "PASS" => styled.green().to_string(),
        "FAIL" | "ERROR" => styled.red().bold().to_string(),
        "UNCERTAIN" | "INCOMPLETE" => styled.yellow().to_string(),
        _ => styled.dim().to_string(),
    }
}
