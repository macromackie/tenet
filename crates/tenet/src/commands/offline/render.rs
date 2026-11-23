use std::io::Write;

use serde_json::json;
use tenet_checks::{
    behavior::{AuditReplayResult, Outcome},
    fixes,
    mechanical,
};

use super::audit::write_replay_results;
use crate::{
    cli::ExitStatus,
    error::CommandError,
    output,
    report_format::{format_dim, format_display, format_marker},
    review::{RepoCoverage, format_coverage_line, write_mechanical},
};

const OFFLINE_VERSION: u64 = 1;

pub(super) struct OfflineOutput<'output> {
    pub behavior: &'output [AuditReplayResult],
    pub coverage: &'output RepoCoverage,
    pub local: &'output [mechanical::Evidence],
    pub native: &'output fixes::Report,
    pub status: ExitStatus,
}

pub(super) fn write_offline_output(
    out: &mut impl Write,
    payload: &OfflineOutput<'_>,
    json_mode: bool,
) -> Result<(), CommandError> {
    if json_mode {
        return write_offline_json(out, payload);
    }
    write_offline_terminal(out, payload)
}

fn count_behavior(rows: &[AuditReplayResult]) -> (usize, usize, usize) {
    let mut passed = 0_usize;
    let mut failed = 0_usize;
    let mut unresolved = 0_usize;
    for row in rows {
        match row.outcome {
            Outcome::Passed => passed = passed.saturating_add(1),
            Outcome::Failed => failed = failed.saturating_add(1),
            Outcome::Unresolved => unresolved = unresolved.saturating_add(1),
        }
    }
    (passed, failed, unresolved)
}

fn write_native_section(out: &mut impl Write, native: &fixes::Report) -> Result<(), CommandError> {
    if native.verified.is_empty() && native.unresolved.is_empty() {
        return Ok(());
    }
    writeln!(out, "{}", output::format_styled("Static checks").bold())?;
    for item in &native.verified {
        let marker = if item.passed {
            format_marker("PASS")
        } else {
            format_marker("FAIL")
        };
        let status = if item.passed { "passed" } else { "failed" };
        writeln!(
            out,
            "  {marker} {} ({})  {}",
            item.contract,
            item.tool,
            format_dim(status),
        )?;
        if let Some(finding) = &item.finding {
            writeln!(out, "    {}", format_display(finding))?;
        }
    }
    for item in &native.unresolved {
        writeln!(
            out,
            "  {} {} ({})  {}",
            format_marker("REVIEW"),
            item.contract,
            item.tool,
            format_display(&item.message),
        )?;
    }
    writeln!(out)?;
    Ok(())
}

fn write_offline_json(
    out: &mut impl Write,
    payload: &OfflineOutput<'_>,
) -> Result<(), CommandError> {
    let (passed, failed, unresolved) = count_behavior(payload.behavior);
    output::write_document(
        out,
        OFFLINE_VERSION,
        json!({
            "mode": "offline-check",
            "native": payload.native,
            "local": payload.local,
            "behavior": payload.behavior,
            "coverage": payload.coverage,
            "summary": {
                "native_verified": payload.native.verified.len(),
                "native_unresolved": payload.native.unresolved.len(),
                "local": payload.local.len(),
                "behavior_passed": passed,
                "behavior_failed": failed,
                "behavior_unresolved": unresolved,
                "requests": 0_usize,
            },
            "exit_code": u8::from(payload.status),
        }),
    )?;
    Ok(())
}

fn write_behavior_section(
    out: &mut impl Write,
    payload: &OfflineOutput<'_>,
) -> Result<(), CommandError> {
    if payload.behavior.is_empty()
        && payload.native.verified.is_empty()
        && payload.native.unresolved.is_empty()
        && payload.local.is_empty()
    {
        writeln!(
            out,
            "  {} No pinned behavior, local, or native checks were found.\n",
            format_marker("PASS"),
        )?;
        return Ok(());
    }
    write_replay_results(out, payload.behavior, "PASS")
}

fn write_offline_terminal(
    out: &mut impl Write,
    payload: &OfflineOutput<'_>,
) -> Result<(), CommandError> {
    let (passed, failed, unresolved) = count_behavior(payload.behavior);
    writeln!(
        out,
        "{}  {}\n",
        output::format_styled("Tenet Offline Check").bold(),
        format_dim(&format!(
            "{} pinned checks \u{b7} zero model requests",
            payload.behavior.len(),
        )),
    )?;
    write_native_section(out, payload.native)?;
    if !payload.local.is_empty() {
        write_mechanical(out, payload.local)?;
        writeln!(out)?;
    }
    write_behavior_section(out, payload)?;
    writeln!(
        out,
        "  {}  {passed} passed \u{b7} {failed} failed \u{b7} {unresolved} unresolved \u{b7} 0 \
         requests",
        output::format_styled("Summary").bold(),
    )?;
    writeln!(
        out,
        "  {}  {}",
        output::format_styled("Coverage").bold(),
        format_coverage_line(payload.coverage),
    )?;
    Ok(())
}
