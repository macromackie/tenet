use tenet_engine::{Coverage, Summary};

use super::Reporter;
use crate::{
    cli::ReporterKind,
    error::CommandError,
    report_format::{self, format_contract_counts, format_counts},
};

impl Reporter<'_> {
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        clippy::float_arithmetic,
        reason = "Elapsed milliseconds are rounded for display and stay below 2^52."
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "The final report preserves its contract, coverage, request, revision and \
                  next-step order."
    )]
    #[expect(
        clippy::cognitive_complexity,
        reason = "The final report keeps failure, revision and next-step output in one order."
    )]
    #[expect(
        clippy::else_if_without_else,
        reason = "needless_else rejects the empty else this lint asks for."
    )]
    pub(super) fn write_summary(
        &mut self,
        summary: Summary,
        revision: Option<u64>,
        pending: &[String],
        coverage: Option<&Coverage>,
    ) -> Result<(), CommandError> {
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
        match coverage.and_then(report_format::format_coverage) {
            Some(coverage_text) => self.term.write_line(&format!(" {coverage_text}"))?,
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
                    " · {failures} failed ({retryable} retryable); a rerun sends only failed \
                     requests"
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
        if let Some(saved_revision) = revision {
            self.term.write_line(&format!(
                " Revision   r{saved_revision} in {}",
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
        Ok(())
    }
}
