use alloc::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use tenet_engine::{
    ContractState,
    MIN_CONFIDENCE,
    Pending,
    review::{self, Current, CurrentContract, Revision},
};

use crate::report_format::{self, Model};

/// The model's conclusion for the row's exact input.
pub(super) fn find_model(entry: &CurrentContract<'_>) -> Option<Model> {
    let conclusion = entry.assessment?.conclusion.as_ref()?;
    Some(Model {
        status: conclusion.status,
        violation: conclusion.violation,
        answer: conclusion.answer,
        min_confidence: entry.min_confidence,
    })
}

/// The row's state as `check` reports it, and which check the changed inputs were captured in.
#[expect(
    clippy::format_push_string,
    reason = "The text is built in memory, where `write!` would add a Result that cannot fail."
)]
pub(super) fn format_state_text(entry: &CurrentContract<'_>) -> (&'static str, String) {
    let threshold = format_threshold_note(entry.min_confidence);
    let (label, mut note) =
        report_format::format_state(&entry.state, find_model(entry), &threshold);
    if entry.state
        == (ContractState::Pending {
            reason: Pending::InputsChanged,
        })
    {
        note.push_str(&format!(" after r{}", entry.selection.rev));
    }
    (label, note)
}

/// How many of each label, in label order: "2 fail · 30 pass".
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Counts are bounded by the contracts of one review."
)]
#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
pub(super) fn format_tally<'label>(labels: impl Iterator<Item = &'label str>) -> String {
    let mut counts = BTreeMap::new();
    for label in labels {
        *counts.entry(label.to_lowercase()).or_insert(0_i32) += 1_i32;
    }
    let counts: Vec<_> = counts
        .iter()
        .map(|(label, count)| format!("{count} {label}"))
        .collect();
    counts.join(" \u{b7} ")
}

/// The name that selects this row in `show` and `record`: the contract, plus the selection's
/// paths when the contract stands in more than one check.
pub(super) fn format_row_name(current: &Current<'_>, entry: &CurrentContract<'_>) -> String {
    let mut name = entry.contract.name.clone();
    let rows = current
        .contracts
        .iter()
        .filter(|contract| contract.contract.name == name)
        .count();
    if rows > 1 {
        for path in &entry.selection.paths {
            name.push(' ');
            name.push_str(&path.display().to_string());
        }
    }
    name
}

/// A revision's selected paths, as a note after the review's name.
pub(super) fn format_paths_note(revision: &Revision) -> String {
    if revision.paths.is_empty() {
        String::new()
    } else {
        format!(" · {}", format_selection_text(revision))
    }
}

pub(super) fn format_selection_text(revision: &Revision) -> String {
    if revision.paths.is_empty() {
        return "(every path)".into();
    }
    revision
        .paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A confidence threshold that differs from the default, which changes how answers are routed.
#[expect(
    clippy::float_cmp_const,
    reason = "The parser stores the default threshold unchanged, so an exact comparison finds it."
)]
pub(super) fn format_threshold_note(min_confidence: Option<f64>) -> String {
    match min_confidence {
        Some(threshold) if threshold != MIN_CONFIDENCE => {
            format!(" · min confidence {threshold}")
        },
        _ => String::new(),
    }
}

pub(super) fn format_span_text(span: &review::Span) -> String {
    match span.lines {
        Some([start, end]) if start == end => format!("{}:{start}", span.path.display()),
        Some([start, end]) => format!("{}:{start}-{end}", span.path.display()),
        None => span.path.display().to_string(),
    }
}

/// The check that captures this selection again against the review's base commit, naming
/// `contract` if given.
#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
pub(super) fn check_command(revision: &Revision, contract: Option<&str>) -> String {
    let mut command = String::from("tenet check");
    for path in &revision.paths {
        command.push(' ');
        command.push_str(&path.display().to_string());
    }
    if let Some(base) = &revision.base_oid {
        command.push_str(" --base ");
        command.push_str(base);
    }
    if let Some(contract) = contract {
        command.push_str(" --contract ");
        command.push_str(contract);
    }
    for request in &revision.context {
        command.push_str(" --context ");
        command.push_str(&request.to_string());
    }
    command
}

#[expect(
    clippy::integer_division,
    reason = "The division rounds down by design: whole spinner frames and whole minutes, hours \
              and days."
)]
#[expect(
    clippy::integer_division_remainder_used,
    reason = "The quotient and remainder pick a spinner frame or a whole time unit, and \
              tokio::select! expands to a remainder for its branch order."
)]
pub(super) fn format_ago(at: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(at, |duration| duration.as_secs());
    match now.saturating_sub(at) {
        0..60 => "just now".into(),
        seconds @ 60..3600 => format!("{}m ago", seconds / 60),
        seconds @ 3600..86400 => format!("{}h ago", seconds / 3600),
        seconds => format!("{}d ago", seconds / 86400),
    }
}
