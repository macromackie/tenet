use std::io::Write;

use serde_json::json;
use tenet_checks::{
    behavior::{AuditFixOutcome, AuditFixResponse, AuditFixStatus},
    fixes,
};

use super::super::super::audit::{StatusRow, write_status_row};
use crate::{
    cli::ExitStatus,
    error::CommandError,
    output,
    report_format::{format_marker, format_painted},
};

const AUDIT_FIX_VERSION: u64 = 1;

struct OutcomeCounts {
    already_passing: usize,
    failed: usize,
    fixed: usize,
    unresolved: usize,
}

/// Write the `tenet fix --from-audit` report as either versioned JSON or terminal output.
pub(super) fn write_fix_report(
    out: &mut impl Write,
    response: &AuditFixResponse,
    changes: &[fixes::Change],
    status: ExitStatus,
    json_mode: bool,
) -> Result<(), CommandError> {
    if json_mode {
        return write_fix_json(out, response, changes, status);
    }
    write_fix_terminal(out, response, changes)
}

fn count_fix_outcomes(outcomes: &[AuditFixOutcome]) -> OutcomeCounts {
    let mut counts = OutcomeCounts {
        already_passing: 0,
        failed: 0,
        fixed: 0,
        unresolved: 0,
    };
    for outcome in outcomes {
        match outcome.status {
            AuditFixStatus::AlreadyPassing => {
                counts.already_passing = counts.already_passing.saturating_add(1);
            },
            AuditFixStatus::Failed => counts.failed = counts.failed.saturating_add(1),
            AuditFixStatus::Fixed => counts.fixed = counts.fixed.saturating_add(1),
            AuditFixStatus::Unresolved => counts.unresolved = counts.unresolved.saturating_add(1),
        }
    }
    counts
}

fn format_fix_badge(status: AuditFixStatus) -> (String, String) {
    match status {
        AuditFixStatus::Fixed => {
            (
                format_marker("PASS"),
                output::format_styled("FIXED").green().bold().to_string(),
            )
        },
        AuditFixStatus::AlreadyPassing => {
            (
                format_marker("PASS"),
                output::format_styled("ALREADY PASSING")
                    .green()
                    .bold()
                    .to_string(),
            )
        },
        AuditFixStatus::Failed => (format_marker("FAIL"), format_painted("FAIL")),
        AuditFixStatus::Unresolved => (format_marker("REVIEW"), format_painted("REVIEW")),
    }
}

fn write_fix_json(
    out: &mut impl Write,
    response: &AuditFixResponse,
    changes: &[fixes::Change],
    status: ExitStatus,
) -> Result<(), CommandError> {
    let counts = count_fix_outcomes(&response.outcomes);
    output::write_document(
        out,
        AUDIT_FIX_VERSION,
        json!({
            "mode": "audit-fix",
            "changes": changes,
            "outcomes": response.outcomes,
            "summary": {
                "fixed": counts.fixed,
                "already_passing": counts.already_passing,
                "failed": counts.failed,
                "unresolved": counts.unresolved,
                "files_updated": changes.len(),
                "requests": response.requests,
                "cost_usd": response.cost_usd,
            },
            "exit_code": u8::from(status),
        }),
    )?;
    Ok(())
}

pub(in crate::commands::fix) fn write_fix_terminal_outcomes(
    out: &mut impl Write,
    response: &AuditFixResponse,
    changes_count: usize,
) -> Result<(), CommandError> {
    let counts = count_fix_outcomes(&response.outcomes);
    if response.outcomes.is_empty() {
        writeln!(
            out,
            "  {} No failing audit witnesses matched for repair.\n",
            format_marker("PASS"),
        )?;
    } else {
        for outcome in &response.outcomes {
            let (marker, label) = format_fix_badge(outcome.status);
            write_status_row(
                out,
                &StatusRow {
                    check: &outcome.check,
                    contract: &outcome.contract,
                    expected: outcome.expected.as_deref(),
                    label,
                    marker,
                    message: &outcome.message,
                    observed: outcome.observed.as_deref(),
                    subject: outcome.subject.as_deref(),
                },
            )?;
        }
    }
    let cost_suffix = response
        .cost_usd
        .map_or_else(String::new, |cost| format!(" · ${cost:.4}"));
    writeln!(
        out,
        "  {}  {} fixed · {} already passing · {} failed · {} unresolved · {changes_count} files \
         updated · {} requests{cost_suffix}",
        output::format_styled("Summary").bold(),
        counts.fixed,
        counts.already_passing,
        counts.failed,
        counts.unresolved,
        response.requests,
    )?;
    Ok(())
}

fn write_fix_terminal(
    out: &mut impl Write,
    response: &AuditFixResponse,
    changes: &[fixes::Change],
) -> Result<(), CommandError> {
    for change in changes {
        writeln!(out, "{}", change.patch)?;
    }
    write_fix_terminal_outcomes(out, response, changes.len())
}
