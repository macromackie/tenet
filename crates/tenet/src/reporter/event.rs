use std::time::Instant;

use tenet_engine::{Event, Stage, Status};

use super::{ContractProgress, Reporter};
use crate::{
    cli::ReporterKind,
    error::CommandError,
    report_format::{format_painted, label_status},
};

/// The results a contract's progress lists as issues.
const ISSUE_STATUSES: [Status; 4] = [
    Status::Fail,
    Status::Uncertain,
    Status::Error,
    Status::Incomplete,
];

impl Reporter<'_> {
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Completed file counts are bounded by the run's selected files."
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "Each event keeps its state transition and corresponding output together."
    )]
    #[expect(
        clippy::excessive_nesting,
        reason = "A verbose result's optional reason stays in the event that records the result."
    )]
    pub(super) fn write_event(&mut self, event: Event) -> Result<(), CommandError> {
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
            } => self.write_contract(contract, summary, conclusion.as_deref(), &state)?,
            Event::Skipped { path, reason } => {
                self.term.write_line(&format!("SKIP {path}: {reason}"))?;
            },
            Event::Summary {
                summary,
                revision,
                pending,
                coverage,
                ..
            } => self.write_summary(summary, revision, &pending, coverage.as_ref())?,
            Event::Error { message } => self.term.write_line(&format!("ERROR {message}"))?,
        }
        Ok(())
    }
}
