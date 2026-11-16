use tenet_engine::{CombinedStatus, ContextSummary, HunkResult, Status};

use super::{format_violation, label_status, request_failure};

const FAVORABLE_STATUSES: [Status; 2] = [Status::Pass, Status::NotApplicable];

pub(crate) fn format_combined(
    status: Option<CombinedStatus>,
    context: Option<&ContextSummary>,
) -> String {
    let label = match status {
        Some(CombinedStatus::Answered) => "answered",
        Some(CombinedStatus::TooLarge) => "too large",
        Some(CombinedStatus::RequestBudget) => "request budget exhausted",
        Some(CombinedStatus::InputUnavailable) => "input unavailable",
        Some(CombinedStatus::RequestFailed) => "request failed",
        Some(CombinedStatus::NotNeeded) => "unaffected input",
        None => "coverage not recorded",
    };
    match context {
        Some(summary) => {
            format!(
                "Combined review  {label} · {} bytes required · {} byte limit",
                summary.input_bytes, summary.limit_bytes
            )
        },
        None => format!("Combined review  {label}"),
    }
}

pub(crate) fn format_hunks(hunks: &[HunkResult]) -> Vec<String> {
    if hunks.is_empty() {
        return Vec::new();
    }
    let total = hunks.iter().map(|hunk| hunk.ranges.len()).sum::<usize>();
    let answered = hunks
        .iter()
        .filter(|hunk| hunk.verification.is_some())
        .map(|hunk| hunk.ranges.len())
        .sum::<usize>();
    let mut lines = vec![format!(
        "Hunks  {answered} of {total} examined · whole-file assessment incomplete"
    )];
    for hunk in hunks
        .iter()
        .filter(|hunk| !FAVORABLE_STATUSES.contains(&hunk.status))
    {
        let ranges = hunk
            .ranges
            .iter()
            .map(|range| {
                let old = range.old.map(|[start, end]| format!("old {start}-{end}"));
                let new = range.new.map(|[start, end]| format!("new {start}-{end}"));
                [old, new]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .collect::<Vec<_>>()
            .join(" \u{b7} ");
        let reason = hunk
            .reason
            .as_deref()
            .unwrap_or("advisory model assessment");
        let failure = hunk
            .failure
            .map(|failed| format!(" · {}", request_failure(failed)))
            .unwrap_or_default();
        lines.push(format!(
            "{ranges} · {}{} · {reason}{failure}",
            label_status(hunk.status).to_lowercase(),
            format_violation(hunk.violation)
        ));
    }
    lines
}
