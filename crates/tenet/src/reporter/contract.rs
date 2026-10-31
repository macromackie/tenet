use core::mem;

use tenet_engine::{
    ContractResult,
    ContractState,
    ContractStatus,
    Pending,
    Status,
    Summary,
    review::Answer,
};

use super::Reporter;
use crate::{
    diagnostics,
    error::CommandError,
    report_format::{self, Model, format_marker, format_painted, request_failure},
};

/// The results that left a check unfinished.
const UNFINISHED_STATUSES: [Status; 2] = [Status::Error, Status::Incomplete];

impl Reporter<'_> {
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Counters are bounded by one run's contracts."
    )]
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        clippy::float_arithmetic,
        reason = "Elapsed milliseconds are rounded for display and stay below 2^52."
    )]
    #[expect(
        clippy::else_if_without_else,
        reason = "needless_else rejects the empty else this lint asks for."
    )]
    #[expect(
        clippy::cognitive_complexity,
        reason = "Recorded, failed and unresolved contract output follows one precedence order."
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "One contract's output follows its conclusion, context and unfinished file \
                  issues in order."
    )]
    pub(super) fn write_contract(
        &mut self,
        contract: String,
        summary: Summary,
        conclusion: Option<&ContractResult>,
        state: &ContractState,
    ) -> Result<(), CommandError> {
        let mut progress = self.active_contracts.remove(&contract).unwrap_or_default();
        let model = conclusion.map(|verdict| {
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
        let unchanged = if conclusion.is_some_and(|verdict| verdict.cached) {
            " \u{b7} unchanged"
        } else {
            ""
        };
        let (label, note) = report_format::format_state(state, model, unchanged);
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
            ContractState::Pending { reason } => *reason == Pending::ModelFailed,
            ContractState::Settled {
                conclusion: settled,
                ..
            } => *settled == ContractStatus::Failed,
        };
        let record = match state {
            ContractState::Settled {
                record: Some(id), ..
            } => Some(id),
            ContractState::Pending { .. } | ContractState::Settled { record: None, .. } => None,
        };
        if let Some(id) = record {
            if failed {
                let reason = format!("Recorded {id}; tenet show {contract} gives its evidence.");
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
                reason: conclusion.map(|verdict| verdict.reason.clone()),
            });
        } else if let Some(unresolved) = conclusion
            && unresolved.status == ContractStatus::Unresolved
        {
            if !unresolved.conflicting_files.is_empty() {
                self.term.write_line(&format!(
                    "  Assessments disagree: {}",
                    unresolved.conflicting_files.join(", ")
                ))?;
            }
            if let Some(error) = &unresolved.error {
                let kind = unresolved
                    .failure
                    .map(|request| format!(" ({})", request_failure(request)))
                    .unwrap_or_default();
                self.term.write_line(&format!("  {error}{kind}"))?;
            } else {
                self.term.write_line(&format!("  {}", unresolved.reason))?;
            }
        } else if let Some(clear) = conclusion
            && clear.status == ContractStatus::Clear
        {
            self.term.write_line(&format!("  {}", clear.reason))?;
        }
        if let Some(context) = conclusion.and_then(|verdict| verdict.context.as_ref())
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
        Ok(())
    }
}
