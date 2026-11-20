mod environment;
mod repository;

use std::{io::Write, path::Path};

use environment::{check_credentials, check_node, check_runtimes, check_sandbox};
use repository::{check_contracts, check_git, check_lockfile, check_state};
use serde::Serialize;
use serde_json::json;

use crate::{
    cli::ExitStatus,
    error::CommandError,
    output,
    report_format::{format_dim, format_marker, format_painted},
};

const DOCTOR_VERSION: u64 = 1;

#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CheckStatus {
    Fail,
    Ok,
    Warn,
}

#[derive(Serialize)]
pub(super) struct DoctorItem {
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub label: &'static str,
    pub name: &'static str,
    pub required: bool,
    pub status: CheckStatus,
}

const fn map_marker(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Ok => "PASS",
        CheckStatus::Warn => "REVIEW",
        CheckStatus::Fail => "FAIL",
    }
}

fn collect_checks(root: &Path, state: &Path) -> Vec<DoctorItem> {
    let (in_git, git_item) = check_git(root);
    vec![
        check_credentials(),
        check_node(),
        check_sandbox(),
        check_runtimes(),
        git_item,
        check_contracts(root),
        check_lockfile(root),
        check_state(root, state, in_git),
    ]
}

fn write_report(
    out: &mut impl Write,
    items: &[DoctorItem],
    failed: usize,
    warnings: usize,
) -> Result<(), CommandError> {
    writeln!(
        out,
        "{}  {}\n",
        output::format_styled("Tenet doctor").bold(),
        format_dim(&format!("v{}", env!("CARGO_PKG_VERSION"))),
    )?;
    let width = items.iter().map(|item| item.label.len()).max().unwrap_or(0);
    for item in items {
        writeln!(
            out,
            "  {} {:width$}  {}",
            format_marker(map_marker(item.status)),
            item.label,
            item.detail,
        )?;
        if let Some(hint) = &item.hint {
            writeln!(
                out,
                "    {:width$}  {} {}",
                "",
                output::format_styled("= help:").cyan(),
                format_dim(hint),
            )?;
        }
    }
    writeln!(out)?;
    match (failed, warnings) {
        (0, 0) => writeln!(out, "  {} Ready to run checks.", format_painted("PASS"))?,
        (0, warn_count) => {
            writeln!(
                out,
                "  {} Ready to run checks ({warn_count} advisory warning(s)).",
                format_painted("PASS")
            )?;
        },
        (fail_count, _) => {
            writeln!(
                out,
                "  {} {fail_count} required check(s) need attention.",
                format_painted("FAIL")
            )?;
        },
    }
    Ok(())
}

pub(super) fn run(root: &Path, state: &Path, json: bool) -> Result<ExitStatus, CommandError> {
    let items = collect_checks(root, state);
    let failed = items
        .iter()
        .filter(|item| item.status == CheckStatus::Fail)
        .count();
    let warnings = items
        .iter()
        .filter(|item| item.status == CheckStatus::Warn)
        .count();
    let exit_status = if failed == 0 {
        ExitStatus::Passed
    } else {
        ExitStatus::Failed
    };
    let mut out = output::open_stdout().lock();
    if json {
        output::write_document(
            &mut out,
            DOCTOR_VERSION,
            json!({
                "ready": failed == 0,
                "failed": failed,
                "warnings": warnings,
                "checks": items,
                "exit_code": u8::from(exit_status),
            }),
        )?;
        return Ok(exit_status);
    }
    write_report(&mut out, &items, failed, warnings)?;
    Ok(exit_status)
}
