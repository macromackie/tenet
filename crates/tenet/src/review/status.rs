mod checks;
mod coverage;

use std::{io::Write as _, path::Path};

pub(crate) use checks::write_mechanical;
use checks::{
    collect_failed_behavior_contracts,
    format_contract_row_state,
    write_status_json,
    write_unchecked,
};
use coverage::write_coverage_section;
pub(crate) use coverage::{
    RepoCoverage,
    compute_coverage,
    discover_all_contracts,
    format_coverage_line,
};
use tenet_checks::mechanical;
use tenet_engine::{
    ContractState,
    Pending,
    Review,
    review::{self, CurrentContract, ReviewError},
};

use super::{
    check_command,
    find_model,
    format_ago,
    format_paths_note,
    format_row_name,
    format_selection_text,
};
use crate::{
    cli::ExitStatus,
    error::CommandError,
    generated,
    output,
    report_format::{format_dim, format_marker, format_painted},
};

#[expect(
    clippy::arithmetic_side_effects,
    reason = "Counts are bounded by the stored review's contracts."
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
    evidence: Option<&Path>,
    json: bool,
    prune_cache: bool,
) -> Result<ExitStatus, CommandError> {
    let mechanical = if let Some(path) = evidence {
        Some(mechanical::verify(root, path)?)
    } else if mechanical::has_local_results(state) {
        Some(mechanical::inspect_local(root, state, None)?)
    } else {
        None
    };
    let coverage = compute_coverage(root, state).ok();
    let has_contracts = coverage
        .as_ref()
        .is_some_and(|map| !map.contracts.is_empty());
    let review = if mechanical.is_some() || has_contracts {
        Review::read(state)?
    } else {
        Review::open_existing(state)?
    };
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
            return write_unchecked(
                &mut out,
                Some(&unchecked.base),
                &unchecked.to_string(),
                mechanical.as_deref(),
                coverage.as_ref(),
                json,
            );
        },
        Err(error @ ReviewError::NoChecks) if mechanical.is_some() || has_contracts => {
            return write_unchecked(
                &mut out,
                None,
                &error.to_string(),
                mechanical.as_deref(),
                coverage.as_ref(),
                json,
            );
        },
        Err(error) => return Err(error.into()),
    };
    let observed = current
        .contracts
        .iter()
        .map(|entry| generated::inspect(root, state, entry, evidence.is_none()))
        .collect::<Result<Vec<_>, _>>()?;
    let generated: Vec<_> = observed.into_iter().flatten().collect();
    let behavior_failed = collect_failed_behavior_contracts(&generated);
    let semantic = review::summarize_overall(current.contracts.iter().map(|entry| &entry.state));
    let status = ExitStatus::from(semantic.max(generated::summarize_status(&generated)));
    if json {
        write_status_json(
            &mut out,
            &current,
            &behavior_failed,
            &generated,
            mechanical.as_deref(),
            coverage.as_ref(),
            status,
        )?;
        return Ok(status);
    }
    writeln!(
        out,
        "{} {}\n",
        output::format_styled(format!(
            "Review {}{}",
            current.latest.review,
            format_paths_note(current.latest),
        ))
        .bold(),
        format_dim(&format!(
            "\u{b7} latest check r{} {}",
            current.latest.rev,
            format_ago(current.latest.at)
        )),
    )?;
    if current.unchanged {
        writeln!(
            out,
            " {}",
            format_dim("The working tree has no changes since the base; nothing to review.")
        )?;
    }
    let width = current
        .contracts
        .iter()
        .map(|entry| entry.contract.name.len())
        .max()
        .unwrap_or(0);
    for entry in &current.contracts {
        let (label, mut note) = format_contract_row_state(entry, &behavior_failed);
        if !entry.selection.is_selecting(&current.latest.paths) {
            note.push_str(&format!(
                " \u{b7} from r{} {}",
                entry.selection.rev,
                format_selection_text(entry.selection)
            ));
        }
        let lower = label.to_lowercase();
        let pad = " ".repeat(10_usize.saturating_sub(lower.len()));
        writeln!(
            out,
            "  {} {:width$}  {}{pad}  {}",
            format_marker(label),
            entry.contract.name,
            format_painted(&lower),
            format_dim(&note),
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
    if let Some(rows) = &mechanical {
        write_mechanical(&mut out, rows)?;
    }
    generated::write_section(&mut out, &generated)?;
    if let Some(map) = &coverage {
        write_coverage_section(&mut out, map)?;
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
    } else if let Some(first) = pending.iter().max_by(|left, right| {
        let left_failed = behavior_failed.contains(left.contract.name.as_str());
        let right_failed = behavior_failed.contains(right.contract.name.as_str());
        left_failed
            .cmp(&right_failed)
            .then_with(|| violation(left).total_cmp(&violation(right)))
    }) {
        writeln!(
            out,
            " Next  tenet show {}",
            format_row_name(&current, first)
        )?;
    }
    Ok(status)
}
