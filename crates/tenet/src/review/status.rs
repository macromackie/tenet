use std::{io::Write as _, path::Path};

use serde_json::json;
use tenet_engine::{
    ContractState,
    Pending,
    Review,
    review::{self, CurrentContract, ReviewError},
};

use super::{
    STATUS_VERSION,
    check_command,
    find_model,
    format_ago,
    format_paths_note,
    format_row_name,
    format_selection_text,
    format_state_text,
};
use crate::{
    cli::ExitStatus,
    error::CommandError,
    output,
    report_format::{format_marker, format_painted},
};

#[expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "Counts are bounded by the stored review's contracts, and `value` is a JSON object \
              the function builds."
)]
#[expect(
    clippy::else_if_without_else,
    reason = "`needless_else` rejects the empty `else` this lint asks for."
)]
#[expect(
    clippy::format_push_string,
    reason = "The text is built in memory, where `write!` would add a Result that cannot fail."
)]
#[expect(
    clippy::future_not_send,
    reason = "The command drives this future on one current-thread runtime and never moves it \
              across threads."
)]
#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
#[expect(
    clippy::shadow_unrelated,
    reason = "Each block binds its own result under the name the following lines read."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "The remaining variants take no action here, and listing each would repeat the enum."
)]
pub(crate) async fn report_status(
    root: &Path,
    state: &Path,
    base: Option<&str>,
    json: bool,
    prune_cache: bool,
) -> Result<ExitStatus, CommandError> {
    let review = Review::open_existing(state)?;
    if prune_cache {
        let pruned = review.prune_answers()?;
        writeln!(
            output::open_stderr(),
            "Removed {} of {} cached answers that this Tenet cannot reuse.",
            pruned.removed,
            pruned.removed + pruned.kept
        )?;
    }
    let mut out = output::open_stdout().lock();
    let current = match review::resolve_current(root, &review, base, None).await {
        Ok(current) => current,
        Err(ReviewError::Unchecked(unchecked)) => {
            // The review against this commit has not begun, so nothing in it is settled.
            if json {
                let message = unchecked.to_string();
                let base = &unchecked.base;
                output::write_document(
                    &mut out,
                    STATUS_VERSION,
                    json!({
                        "base": base,
                        "revision": null,
                        "contracts": [],
                        "exit_code": u8::from(ExitStatus::Pending),
                        "message": message,
                    }),
                )?;
            } else {
                writeln!(out, "{unchecked}")?;
            }
            return Ok(ExitStatus::Pending);
        },
        Err(error) => return Err(error.into()),
    };
    let status = ExitStatus::from(review::summarize_overall(
        current.contracts.iter().map(|contract| &contract.state),
    ));
    if json {
        let contracts = current
            .contracts
            .iter()
            .map(|entry| {
                let mut value = serde_json::to_value(&entry.state)?;
                value["contract"] = json!(entry.contract.name);
                value["path"] = json!(entry.contract.path);
                value["selection"] =
                    json!({"rev": entry.selection.rev, "paths": entry.selection.paths});
                value["input"] = json!(entry.input);
                value["assessed_in"] = json!(entry.assessment.map(|assessment| assessment.rev));
                value["model"] = json!(find_model(entry).map(|verdict| verdict.status));
                value["model_violation"] =
                    json!(find_model(entry).and_then(|verdict| verdict.violation));
                value["model_answer"] = json!(find_model(entry).and_then(|verdict| verdict.answer));
                value["min_confidence"] = json!(entry.min_confidence);
                Ok(value)
            })
            .collect::<Result<Vec<_>, CommandError>>()?;
        output::write_document(
            &mut out,
            STATUS_VERSION,
            json!({
                "review": current.latest.review,
                "base": current.latest.base_oid,
                "revision": current.latest.rev,
                "unchanged": current.unchanged,
                "contracts": contracts,
                "deleted_contracts": current.deleted,
                "exit_code": u8::from(status),
            }),
        )?;
        return Ok(status);
    }
    writeln!(
        out,
        "Review {}{} · latest check r{} {}\n",
        current.latest.review,
        format_paths_note(current.latest),
        current.latest.rev,
        format_ago(current.latest.at)
    )?;
    if current.unchanged {
        writeln!(
            out,
            " The working tree has no changes since the base; nothing to review."
        )?;
    }
    let width = current
        .contracts
        .iter()
        .map(|entry| entry.contract.name.len())
        .max()
        .unwrap_or(0);
    for entry in &current.contracts {
        let (label, mut note) = format_state_text(entry);
        if !entry.selection.is_selecting(&current.latest.paths) {
            note.push_str(&format!(
                " · from r{} {}",
                entry.selection.rev,
                format_selection_text(entry.selection)
            ));
        }
        writeln!(
            out,
            " {} {:width$}  {:10}  {note}",
            format_marker(label),
            entry.contract.name,
            format_painted(&label.to_lowercase()),
        )?;
    }
    for path in &current.deleted {
        writeln!(
            out,
            " {} {}  deleted by this change; its requirement no longer applies",
            format_marker("DELETED"),
            path.display()
        )?;
    }
    let pending: Vec<_> = current
        .contracts
        .iter()
        .filter(|contract| contract.state.is_pending())
        .collect();
    let recorded = current
        .contracts
        .iter()
        .filter(|contract| {
            matches!(
                contract.state,
                ContractState::Settled {
                    record: Some(_),
                    ..
                }
            )
        })
        .count();
    let settled = current.contracts.len() - pending.len();
    let by = if settled > 0 {
        format!(" ({} by model, {recorded} by record)", settled - recorded)
    } else {
        String::new()
    };
    writeln!(out, "\n {settled} settled{by} · {} pending", pending.len())?;
    let outdated = pending.iter().find(|contract| {
        matches!(
            contract.state,
            ContractState::Pending {
                reason: Pending::InputsChanged | Pending::NotChecked | Pending::NotAssessed
            }
        )
    });
    let violation = |contract: &CurrentContract<'_>| {
        find_model(contract)
            .and_then(|verdict| verdict.violation)
            .unwrap_or(-1.0_f64)
    };
    if let Some(outdated) = outdated {
        // A contract no check assessed needs a check of every contract, not of the one named.
        let contract = match outdated.state {
            ContractState::Pending {
                reason: Pending::NotChecked,
            } => None,
            _ => outdated.selection.contract.as_deref(),
        };
        writeln!(
            out,
            " Next  {}",
            check_command(outdated.selection, contract)
        )?;
    } else if let Some(first) = pending
        .iter()
        .max_by(|left, right| violation(left).total_cmp(&violation(right)))
    {
        writeln!(
            out,
            " Next  tenet show {}",
            format_row_name(&current, first)
        )?;
    }
    Ok(status)
}
