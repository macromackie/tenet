use std::io::Write as _;

use serde_json::{Value, json};
use tenet_engine::{Summary, mechanical::Evidence, review::OverallStatus};

use super::{FinalReview, contracts::Row, replay};
use crate::{
    agent_checks::Finding,
    cli::ExitStatus,
    error::CommandError,
    generated,
    output,
    review::write_mechanical,
};

#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "The final review assembles this internal report in its parent module."
)]
pub(super) struct Report<'run> {
    pub(super) checks: Vec<Evidence>,
    pub(super) contracts: Vec<Row<'run>>,
    pub(super) errors: Vec<String>,
    pub(super) generated: Vec<Finding>,
    pub(super) revision: Option<u64>,
    pub(super) source_error: Option<String>,
    pub(super) status: OverallStatus,
    pub(super) summary: Summary,
}

impl Report<'_> {
    fn build_document(&self, review: &FinalReview<'_>, replay: &str) -> Value {
        let rows: Vec<_> = self.contracts.iter().map(Row::build_document).collect();
        json!({
            "type": "final",
            "run": review.run.borrow().as_ref(),
            "mode": review.plan.mode,
            "base": review.plan.base,
            "head": review.plan.head,
            "contracts": rows,
            "checks": self.checks,
            "behavior_checks": self.generated,
            "generation": generated::summarize_generation(&self.generated),
            "revision": self.revision,
            "paths": review.plan.selected_paths,
            "coverage": review.plan.coverage,
            "excluded": review.plan.excluded_paths,
            "check_source": if review.check.evidence.is_some() {"imported"} else {"local"},
            "summary": self.summary,
            "requests": review.requests.borrow().as_slice(),
            "source_error": self.source_error,
            "errors": self.errors,
            "status": format_status(self.status),
            "exit_code": u8::from(ExitStatus::from(self.status)),
            "replay": replay
        })
    }

    pub(super) fn write(&self, review: &FinalReview<'_>) -> Result<ExitStatus, CommandError> {
        let exit = ExitStatus::from(self.status);
        let replay = replay::build_command(review.check, review.plan, review.state, None);
        if review.is_json() {
            output::write_document(
                &mut output::open_stdout().lock(),
                1,
                self.build_document(review, &replay),
            )?;
        } else {
            let mut out = output::open_stdout().lock();
            writeln!(
                out,
                "Final verification {} (contracts {}, local checks {})",
                format_status(self.status),
                self.contracts.len(),
                self.checks.len()
            )?;
            for row in &self.contracts {
                row.write(&mut out)?;
            }
            write_mechanical(&mut out, &self.checks)?;
            generated::write_rows(&mut out, &self.generated)?;
            for error in self.source_error.iter().chain(&self.errors) {
                writeln!(out, "  {}", error.escape_debug())?;
            }
            if exit != ExitStatus::Passed {
                writeln!(out, "Replay    {replay}")?;
            }
        }
        Ok(exit)
    }
}

impl FinalReview<'_> {
    pub(in crate::commands) fn report_stop(
        &self,
        mut summary: Summary,
        error: Option<&CommandError>,
    ) -> Result<ExitStatus, CommandError> {
        let status = if error.is_some() {
            OverallStatus::Error
        } else {
            OverallStatus::Cancelled
        };
        let exit = ExitStatus::from(status);
        let errors: Vec<_> = error.iter().map(ToString::to_string).collect();
        summary.cancelled = status == OverallStatus::Cancelled;
        summary.elapsed_ms = self.started.elapsed().as_millis();
        if self.is_json() {
            output::write_document(
                &mut output::open_stdout().lock(),
                1,
                json!({
                    "type": "final",
                    "run": self.run.borrow().as_ref(),
                    "contracts": [],
                    "checks": [],
                    "summary": summary,
                    "requests": self.requests.borrow().as_slice(),
                    "errors": errors,
                    "status": format_status(status),
                    "exit_code": u8::from(exit)
                }),
            )?;
        } else {
            writeln!(
                output::open_stderr(),
                "Final verification {}.",
                format_status(status)
            )?;
            for message in errors {
                writeln!(output::open_stderr(), "  {}", message.escape_debug())?;
            }
        }
        Ok(exit)
    }
}

const fn format_status(status: OverallStatus) -> &'static str {
    match status {
        OverallStatus::Passed => "passed",
        OverallStatus::Failed => "failed",
        OverallStatus::Error => "error",
        OverallStatus::Unresolved => "unresolved",
        OverallStatus::Pending => "pending",
        OverallStatus::Cancelled => "cancelled",
    }
}
