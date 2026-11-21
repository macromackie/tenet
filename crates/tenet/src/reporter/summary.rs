use alloc::collections::BTreeMap;

use tenet_engine::{ContextProfile, Coverage, Summary};

use super::Reporter;
use crate::{
    cli::ReporterKind,
    error::CommandError,
    report_format::{
        format_contract_counts,
        format_counts,
        format_coverage,
        format_dim,
        format_duration,
        format_painted,
        quote_argument,
    },
};

const fn describe_exit_code(exit_code: u8) -> &'static str {
    match exit_code {
        1 => " (exit 1: violation found)",
        3 => " (exit 3: unresolved review)",
        4 => " (exit 4: pending review)",
        _ => "",
    }
}

fn format_tally(counts: &BTreeMap<&'static str, usize>) -> String {
    let total: usize = counts.values().sum();
    let breakdown = counts
        .iter()
        .map(|(label, count)| format!("{count} {}", format_painted(label)))
        .collect::<Vec<_>>()
        .join(" \u{b7} ");
    format!("{breakdown} {}", format_dim(&format!("({total})")))
}

fn format_size_limit_hint(
    profile: ContextProfile,
    oversized_files: usize,
    oversized_combined: usize,
) -> Option<String> {
    if profile != ContextProfile::Standard || (oversized_files == 0 && oversized_combined == 0) {
        return None;
    }
    let scope = match (oversized_files, oversized_combined) {
        (files, 0) => format!("{files} file check(s)"),
        (0, combined) => format!("{combined} combined scope(s)"),
        (files, combined) => format!("{files} file check(s) and {combined} combined scope(s)"),
    };
    Some(format!(
        "{scope} exceeded the 64 KiB standard limit; rerun with --context-profile long"
    ))
}

impl Reporter<'_> {
    #[expect(
        clippy::too_many_lines,
        reason = "The summary renders coverage, tallies, timing, revision, and next-step hints in \
                  one clear order."
    )]
    fn write_compact_summary(
        &self,
        summary: Summary,
        revision: Option<u64>,
        pending: &[String],
        coverage: Option<&Coverage>,
        exit_code: u8,
    ) -> Result<(), CommandError> {
        if !self.contract_statuses.is_empty() || !self.behavior_statuses.is_empty() {
            self.term.write_line("")?;
        }
        if let Some(message) = coverage.and_then(format_coverage) {
            self.term
                .write_line(&format!("  {}", format_dim(&message)))?;
        }
        for path in &self.deleted {
            self.term
                .write_line(&format!("  Removed contract  {}", path.display()))?;
        }
        if summary.cancelled {
            self.term
                .write_line("  Cancelled. Remaining checks did not finish.")?;
        }
        match self.contract_statuses.is_empty() {
            false => {
                self.term.write_line(&format!(
                    "  {}  {}",
                    format_dim("Contracts"),
                    format_tally(&self.contract_statuses)
                ))?;
            },
            true if self.contracts.is_empty()
                && coverage.is_none_or(|diff| diff.changed_files == 0) =>
            {
                self.term.write_line("No contracts to check.")?;
            },
            true => {},
        }
        if !self.behavior_statuses.is_empty() {
            self.term.write_line(&format!(
                "  {}  {}",
                format_dim("   Checks"),
                format_tally(&self.behavior_statuses)
            ))?;
        }
        if !self.contract_statuses.is_empty() || !self.behavior_statuses.is_empty() {
            self.term.write_line(&format!(
                "  {}  {}",
                format_dim(" Duration"),
                format_duration(summary.elapsed_ms)
            ))?;
        }
        if let Some(number) = revision {
            let events = self.state.join("events.jsonl");
            self.term.write_line(&format!(
                "  {}  {}",
                format_dim("   Review"),
                format_dim(&format!("r{number} in {}", events.display()))
            ))?;
        }
        if let Some(hint) = format_size_limit_hint(
            self.context_profile,
            self.oversized_files,
            self.oversized_combined,
        ) {
            self.term
                .write_line(&format!("  {}  {hint}", format_dim("     Hint")))?;
        }
        let exit_note = format_dim(describe_exit_code(exit_code));
        match (pending.first(), revision.is_some(), exit_note.is_empty()) {
            (Some(name), true, _) => {
                self.term.write_line(&format!(
                    "  {}  tenet show {}{exit_note}",
                    format_dim("     Next"),
                    quote_argument(name),
                ))?;
            },
            (Some(_), false, _) => {
                self.term.write_line(&format!(
                    "  {}  Run outside the snapshot to record a review.{exit_note}",
                    format_dim("     Next"),
                ))?;
            },
            (None, _, false) => {
                self.term.write_line(&format!(
                    "  {}  {exit_code}{exit_note}",
                    format_dim("     Exit"),
                ))?;
            },
            (None, _, true) => {},
        }
        Ok(())
    }

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
        exit_code: u8,
    ) -> Result<(), CommandError> {
        self.running = false;

        if self.kind == ReporterKind::Default {
            return self.write_compact_summary(summary, revision, pending, coverage, exit_code);
        }
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
        match coverage.and_then(format_coverage) {
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
        if let Some(hint) = format_size_limit_hint(
            self.context_profile,
            self.oversized_files,
            self.oversized_combined,
        ) {
            self.term.write_line(&format!(" Hint       {hint}"))?;
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
