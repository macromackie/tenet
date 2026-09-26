use std::{collections::BTreeMap, time::Instant};

use anyhow::Result;
use console::Term;
use serde::Serialize;
use tenet_engine::{ContractStatus, Event, Stage, Summary};

use crate::{
    cli::ReporterKind,
    progress::Progress,
    report_format::{marker, paint},
};

#[derive(Serialize)]
pub(crate) struct CaseResult {
    pub case: String,
    pub expected: BTreeMap<String, ContractStatus>,
    pub actual: BTreeMap<String, ContractStatus>,
    pub error: Option<String>,
    pub summary: Summary,
}

impl CaseResult {
    pub(crate) fn matched(&self) -> bool {
        self.error.is_none() && self.expected == self.actual
    }
    pub(crate) fn matches(&self) -> usize {
        if self.error.is_some() {
            return 0;
        }
        self.expected
            .iter()
            .filter(|(name, status)| self.actual.get(*name) == Some(*status))
            .count()
    }
}

#[derive(Default, Serialize)]
pub(crate) struct Totals {
    pub matched: usize,
    pub mismatched: usize,
    pub errors: usize,
    pub expectations_matched: usize,
    pub expectations_mismatched: usize,
    pub requests: usize,
    pub cancelled: bool,
}

impl Totals {
    pub(crate) fn add(&mut self, result: &CaseResult) {
        if result.error.is_some() {
            self.errors += 1;
        } else if result.matched() {
            self.matched += 1;
        } else {
            self.mismatched += 1;
        }
        if result.error.is_none() {
            self.expectations_matched += result.matches();
            self.expectations_mismatched += result.expected.len() - result.matches();
        }
    }
    pub(crate) fn exit_code(&self, strict: bool) -> u8 {
        if self.cancelled {
            130
        } else if self.errors > 0 {
            2
        } else {
            u8::from(strict && self.mismatched > 0)
        }
    }
}

pub(crate) struct Display {
    term: Term,
    progress: Progress,
    kind: ReporterKind,
    started: Instant,
    case: String,
    contract: String,
    done: usize,
    files: usize,
    completing: bool,
    total: usize,
    pub totals: Totals,
    details: Vec<String>,
}

impl Display {
    pub(crate) fn new(kind: ReporterKind, total: usize) -> Self {
        Self {
            term: Term::buffered_stderr(),
            progress: Progress::default(),
            kind,
            started: Instant::now(),
            case: String::new(),
            contract: String::new(),
            done: 0,
            files: 0,
            completing: false,
            total,
            totals: Totals::default(),
            details: Vec::new(),
        }
    }
    pub(crate) fn live(&self) -> bool {
        self.kind == ReporterKind::Default
            && self.term.is_term()
            && std::env::var_os("CI").is_none()
            && std::env::var("TERM").as_deref() != Ok("dumb")
    }
    pub(crate) fn start(&mut self, case: &str) -> Result<()> {
        self.case = case.into();
        self.contract.clear();
        self.done = 0;
        self.files = 0;
        self.refresh()
    }
    pub(crate) fn event(&mut self, event: &Event) -> Result<()> {
        match event {
            Event::ContractStarted {
                contract, total, ..
            } => {
                self.contract = contract.clone();
                self.files = *total;
                self.done = 0;
                self.completing = false;
                self.refresh()?;
            }
            Event::Result { result } => {
                self.done += 1;
                if self.kind == ReporterKind::Verbose {
                    self.term.write_line(&format!(
                        "   {} > {} > {}: {:?}",
                        self.case, result.contract, result.path, result.status
                    ))?;
                }
            }
            Event::StageStarted { stage, .. } => {
                self.totals.requests += 1;
                self.completing = *stage == Stage::Completeness;
            }
            _ => {}
        }
        self.term.flush()?;
        Ok(())
    }
    pub(crate) fn finish(&mut self, result: &CaseResult) -> Result<()> {
        self.totals.add(result);
        if self.kind == ReporterKind::Jsonl {
            return Ok(());
        }
        self.progress.clear(&self.term)?;
        let state = if result.error.is_some() {
            "ERROR"
        } else if result.matched() {
            "PASS"
        } else {
            "UNRESOLVED"
        };
        self.term.write_line(&format!(
            " {} {} ({}/{} expectations matched) {:.1}s",
            marker(state),
            result.case,
            result.matches(),
            result.expected.len(),
            result.summary.elapsed_ms as f64 / 1000.0
        ))?;
        if let Some(error) = &result.error {
            self.details.push(format!(" {}\n   {error}", result.case));
        } else if !result.matched() {
            let mut detail = format!(" {}", result.case);
            for (name, expected) in &result.expected {
                if result.actual.get(name) != Some(expected) {
                    detail.push_str(&format!(
                        "\n   {name}: expected {}, received {}",
                        serde_json::to_value(expected)?
                            .as_str()
                            .unwrap_or("unknown"),
                        result
                            .actual
                            .get(name)
                            .map(serde_json::to_value)
                            .transpose()?
                            .and_then(|v| v.as_str().map(str::to_owned))
                            .unwrap_or_else(|| "missing".into())
                    ));
                }
            }
            self.details.push(detail);
        }
        self.case.clear();
        self.contract.clear();
        self.refresh()?;
        self.term.flush()?;
        Ok(())
    }
    pub(crate) fn refresh(&mut self) -> Result<()> {
        if !self.live() {
            return Ok(());
        }
        let active = if self.case.is_empty() {
            String::new()
        } else if self.contract.is_empty() {
            self.case.clone()
        } else {
            format!(
                "{} > {} {}/{} files{}",
                self.case,
                self.contract,
                self.done,
                self.files,
                if self.completing {
                    " · assessing contract"
                } else {
                    ""
                }
            )
        };
        self.progress.draw(
            &mut self.term,
            &active,
            &format!(
                "Cases  {} matched | {} mismatched | {} errors ({})",
                self.totals.matched, self.totals.mismatched, self.totals.errors, self.total
            ),
            self.started.elapsed(),
        )?;
        Ok(())
    }
    pub(crate) fn summary(&mut self) -> Result<()> {
        if self.kind == ReporterKind::Jsonl {
            return Ok(());
        }
        self.progress.clear(&self.term)?;
        for detail in &self.details {
            self.term.write_line(&format!("\n{detail}"))?;
        }
        self.term.write_line(&format!("\n Cases         {} matched | {} mismatched | {} errors ({})\n Expectations  {} matched | {} mismatched\n Requests      {}\n Duration      {:.2}s",
            self.totals.matched, self.totals.mismatched, self.totals.errors, self.total,
            self.totals.expectations_matched, self.totals.expectations_mismatched,
            self.totals.requests, self.started.elapsed().as_secs_f64()))?;
        if self.totals.cancelled {
            self.term.write_line(&paint("Cancelled"))?;
        }
        self.term.flush()?;
        Ok(())
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        let _ = self.progress.clear(&self.term);
        let _ = self.term.flush();
    }
}
