use tenet_checks::behavior::{Finding, Outcome};
use tenet_engine::{
    ContractResult,
    ContractState,
    ContractStatus,
    Pending,
    review::{Checked, Concluded},
};

use super::{ContractProgress, Reporter};
use crate::{
    cli::ReporterKind,
    error::CommandError,
    generated,
    report_format::{
        DiagnosticBlock,
        format_dim,
        format_display,
        format_marker,
        format_painted,
        format_violation,
        quote_argument,
        write_diagnostic_block,
    },
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
        let title = self
            .contracts
            .iter()
            .find(|candidate| candidate.name == contract)
            .map_or(contract, |candidate| candidate.message.as_str());
        let Some((label, detail)) = describe_review(state, verdict) else {
            return self.write_passed_contract(title, state);
        };
        if label == "REVIEWED" {
            self.term.write_line(&format!(
                "  {} {}  {}  {}",
                format_marker(label),
                format_painted(label),
                format_display(title),
                format_dim(&format_display(detail)),
            ))?;
            return Ok(());
        }
        let score = verdict
            .and_then(|result| result.violation)
            .map(|share| format_violation(Some(share)));
        let location = progress.issues.first().map(|issue| issue.path.as_str());
        let help = if matches!(
            state,
            ContractState::Pending {
                reason: Pending::ContractChanged
            }
        ) {
            format!("tenet check --full --contract {}", quote_argument(contract))
        } else {
            format!("tenet show {}", quote_argument(contract))
        };
        write_diagnostic_block(
            &mut self.term,
            &DiagnosticBlock {
                help: Some(&help),
                label,
                location,
                message: detail,
                meta: score.as_deref(),
                title,
            },
        )?;
        Ok(())
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Each planned contract and behavior check row contributes once."
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
            self.behavior_statuses.clear();
            for row in rows {
                let label = match row.outcome {
                    Outcome::Passed => "passed",
                    Outcome::Failed => "failed",
                    Outcome::Unresolved => "for review",
                };
                *self.behavior_statuses.entry(label).or_default() += 1;
            }
        }
        generated::emit(rows, self.kind)
    }

    fn write_passed_contract(
        &self,
        title: &str,
        state: &ContractState,
    ) -> Result<(), CommandError> {
        if matches!(
            state,
            ContractState::Settled {
                conclusion: ContractStatus::Verified
                    | ContractStatus::Preserved
                    | ContractStatus::Clear,
                ..
            }
        ) {
            self.term.write_line(&format!(
                "  {} {}  {}",
                format_marker("PASS"),
                format_painted("PASS"),
                format_display(title)
            ))?;
        }
        Ok(())
    }
}
