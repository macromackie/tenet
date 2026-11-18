use alloc::collections::BTreeMap;
use std::{io, path::Path};

use serde_json::{Value, json};
use tenet_engine::{
    ContractState,
    EvidencePacket,
    Pending,
    Plan,
    review::{Checked, Concluded},
};

use super::replay;
use crate::{cli::Check, report_format};

pub(super) struct Row<'run> {
    assessed: Option<&'run Concluded>,
    current: Option<String>,
    name: &'run str,
    path: &'run Path,
    replay: String,
    state: ContractState,
}

impl Row<'_> {
    pub(super) fn build_document(&self) -> Value {
        let mut value = json!(self.state);
        if let Some(fields) = value.as_object_mut() {
            fields.extend([
                ("contract".into(), json!(self.name)),
                ("path".into(), json!(self.path)),
                ("input".into(), json!(self.assessed.map(|row| &row.input))),
                ("current_input".into(), json!(self.current)),
                ("scope".into(), json!(self.assessed.map(|row| row.scope))),
                (
                    "assessment".into(),
                    json!(self.assessed.and_then(|row| row.conclusion.as_ref())),
                ),
                ("next_action".into(), json!(self.describe_next_action())),
                ("replay".into(), json!(self.replay)),
            ]);
        }
        value
    }

    const fn describe_next_action(&self) -> Option<&'static str> {
        let ContractState::Pending { reason } = self.state else {
            return None;
        };
        Some(match reason {
            Pending::InputsChanged | Pending::NotChecked => {
                "Rerun the final check on the current source."
            },
            Pending::NotAssessed => {
                "Run with a model and an available request budget, or inspect and record this \
                 input."
            },
            Pending::ContractChanged => {
                "Run a full check of the changed contract and record a conclusion for this diff."
            },
            Pending::ModelFailed => {
                "Inspect the reported finding, then repair it or record a supported conclusion."
            },
            Pending::ModelUnresolved => {
                "Inspect the assessment for missing evidence or an exhausted budget. Rerun with \
                 the required evidence or record an unresolved conclusion."
            },
        })
    }

    pub(super) const fn state(&self) -> &ContractState {
        &self.state
    }

    pub(super) fn write(&self, out: &mut impl io::Write) -> io::Result<()> {
        let model = self
            .assessed
            .and_then(|row| row.conclusion.as_ref())
            .map(|evidence| {
                report_format::Model {
                    status: evidence.status,
                    violation: evidence.violation,
                    answer: evidence.answer,
                    min_confidence: None,
                }
            });
        let (label, note) = report_format::format_state(&self.state, model, "");
        writeln!(out, "  {}   {label}   {note}", self.name)?;
        if let Some(action) = self.describe_next_action() {
            if let Some(conclusion) = self.assessed.and_then(|row| row.conclusion.as_ref()) {
                writeln!(out, "    {}", conclusion.reason.escape_debug())?;
            }
            writeln!(out, "    {action}")?;
            writeln!(out, "    Replay    {}", self.replay)?;
        }
        Ok(())
    }
}

/// Return the union of the original and final selections. Missing or changed evidence never
/// inherits a favorable conclusion. An already unresolved assessment retains its explanation.
pub(super) fn compare<'run>(
    original: &'run Plan,
    checked: &'run Checked,
    latest: Option<(&'run Plan, &EvidencePacket<'_>)>,
    check: &Check,
    state: &Path,
) -> Vec<Row<'run>> {
    let mut rows = BTreeMap::new();
    for (contract, assessed) in original.contracts.iter().zip(&checked.contracts) {
        drop(rows.insert(
            contract.name.as_str(),
            Row {
                assessed: Some(assessed),
                current: None,
                name: &contract.name,
                path: &contract.path,
                replay: replay::build_command(check, original, state, Some(&contract.name)),
                state: ContractState::Pending {
                    reason: Pending::InputsChanged,
                },
            },
        ));
    }
    if let Some((plan, evidence)) = latest {
        for contract in &plan.contracts {
            let input = evidence.derive_input_key(contract);
            let entry = rows.entry(contract.name.as_str()).or_insert_with(|| {
                Row {
                    assessed: None,
                    current: None,
                    name: &contract.name,
                    path: &contract.path,
                    replay: replay::build_command(check, plan, state, Some(&contract.name)),
                    state: ContractState::Pending {
                        reason: Pending::NotChecked,
                    },
                }
            });
            if let Some(assessed) = entry.assessed
                && assessed.input == input
                && original.base == plan.base
                && (evidence.has_complete_identity(contract)
                    || assessed.state.is_pending()
                    || matches!(
                        assessed.state,
                        ContractState::Settled {
                            conclusion: tenet_engine::ContractStatus::Unresolved,
                            ..
                        }
                    ))
            {
                entry.state = assessed.state.clone();
            }
            entry.current = Some(input);
        }
    }
    rows.into_values().collect()
}
