use tenet_engine::{
    ContractResult,
    ContractState,
    ContractStatus,
    Coverage,
    Pending,
    Summary,
    review::{Checked, Concluded},
};

use super::{ContractProgress, Reporter};
use crate::{
    agent_checks::{Finding, Outcome},
    cli::ReporterKind,
    error::CommandError,
    generated,
    report_format::{format_coverage, format_painted, quote_argument},
};

const PENDING_LABELS: &[&str] = &["failed", "for review"];

/// Labels count only conclusions reached for the selected scope.
const fn count_label(state: &ContractState) -> &'static str {
    match state {
        ContractState::Pending { .. } => "for review",
        ContractState::Settled { conclusion, .. } => {
            match conclusion {
                ContractStatus::Verified | ContractStatus::Preserved => "passed",
                ContractStatus::Clear => "clear in selected files",
                ContractStatus::Unaffected => "unaffected",
                ContractStatus::Failed => "failed",
                ContractStatus::Unresolved => "unresolved",
            }
        },
    }
}

fn describe_review<'result>(
    state: &ContractState,
    verdict: Option<&'result ContractResult>,
) -> Option<(&'static str, &'result str)> {
    match state {
        ContractState::Pending { reason } => Some(("REVIEW", describe_pending(*reason, verdict))),
        ContractState::Settled {
            conclusion: ContractStatus::Failed,
            ..
        } => Some(("FAIL", "The reviewer confirmed a violation.")),
        ContractState::Settled {
            conclusion: ContractStatus::Unresolved,
            ..
        } => Some(("REVIEW", "The reviewer recorded missing evidence.")),
        ContractState::Settled { conclusion, record } => {
            (record.is_some() && verdict.is_some_and(|result| result.status != *conclusion))
                .then_some(("REVIEWED", "The reviewer settled the model's concern."))
        },
    }
}

fn describe_pending(pending: Pending, verdict: Option<&ContractResult>) -> &str {
    let reason = verdict.map(|result| result.reason.as_str());
    match pending {
        Pending::ModelFailed => reason.unwrap_or("The model flagged a possible violation."),
        Pending::ModelUnresolved => {
            verdict
                .and_then(|result| result.error.as_deref())
                .or(reason)
                .unwrap_or("The available evidence did not settle this contract.")
        },
        Pending::NotChecked | Pending::NotAssessed => "This contract still needs a check.",
        Pending::InputsChanged => "The code changed after the last check.",
        Pending::ContractChanged => "The contract changed and needs a full check.",
    }
}

fn label_with_checks(contract: &Concluded, rows: &[Finding]) -> &'static str {
    let mut label = count_label(&contract.state);
    for row in rows.iter().filter(|row| row.contract == contract.contract) {
        match row.outcome {
            Outcome::Failed => return "failed",
            Outcome::Unresolved if !PENDING_LABELS.contains(&label) => {
                label = "unresolved";
            },
            Outcome::Passed | Outcome::Unresolved => {},
        }
    }
    label
}

impl Reporter<'_> {
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "One count per completed contract."
    )]
    pub(super) fn write_compact_contract(
        &mut self,
        contract: &str,
        verdict: Option<&ContractResult>,
        state: &ContractState,
        progress: &ContractProgress,
    ) -> Result<(), CommandError> {
        *self
            .contract_statuses
            .entry(count_label(state))
            .or_default() += 1;
        let Some((label, detail)) = describe_review(state, verdict) else {
            return Ok(());
        };
        let path = progress
            .issues
            .first()
            .map(|issue| format!("  {}", issue.path.escape_debug()))
            .unwrap_or_default();
        let title = self
            .contracts
            .iter()
            .find(|candidate| candidate.name == contract)
            .map_or(contract, |candidate| candidate.message.as_str());
        self.term.write_line(&format!(
            "  {}  {}{path}",
            format_painted(label),
            title.escape_debug()
        ))?;
        self.term
            .write_line(&format!("        {}", detail.escape_debug()))?;
        if matches!(
            state,
            ContractState::Pending {
                reason: Pending::ContractChanged
            }
        ) {
            self.term.write_line(&format!(
                "        tenet check --full --contract {}",
                quote_argument(contract)
            ))?;
        }
        Ok(())
    }

    pub(super) fn write_compact_summary(
        &self,
        summary: Summary,
        revision: Option<u64>,
        pending: &[String],
        coverage: Option<&Coverage>,
    ) -> Result<(), CommandError> {
        if let Some(message) = coverage.and_then(format_coverage) {
            self.term.write_line(&message)?;
        }
        for path in &self.deleted {
            self.term
                .write_line(&format!("Removed contract  {}", path.display()))?;
        }
        if summary.cancelled {
            self.term
                .write_line("Cancelled. Remaining checks did not finish.")?;
        }
        let counts = self
            .contract_statuses
            .iter()
            .map(|(label, count)| format!("{count} {}", format_painted(label)))
            .collect::<Vec<_>>()
            .join(" \u{b7} ");
        match counts.is_empty() {
            false => self.term.write_line(&counts)?,
            true if self.contracts.is_empty()
                && coverage.is_none_or(|diff| diff.changed_files == 0) =>
            {
                self.term.write_line("No contracts to check.")?;
            },
            true => {},
        }
        if let Some(number) = revision {
            self.term.write_line(&format!(
                "Saved review r{number} in {}",
                self.state.join("events.jsonl").display()
            ))?;
        }
        if let Some(name) = pending.first() {
            if revision.is_some() {
                self.term
                    .write_line(&format!("Inspect: tenet show {}", quote_argument(name)))?;
            } else {
                self.term
                    .write_line("Run outside the snapshot to record a review.")?;
            }
        }
        Ok(())
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Each planned contract contributes once."
    )]
    pub(crate) fn write_generated(
        &mut self,
        rows: &[Finding],
        checked: &Checked,
    ) -> Result<(), CommandError> {
        if rows.is_empty() {
            return Ok(());
        }
        if self.kind == ReporterKind::Default {
            self.contract_statuses.clear();
            for contract in &checked.contracts {
                let label = label_with_checks(contract, rows);
                *self.contract_statuses.entry(label).or_default() += 1;
            }
        }
        generated::emit(rows, self.kind)
    }
}
