use alloc::collections::BTreeMap;
use std::io::{self, Write};

use serde_json::json;
use tenet_checks::behavior::{Finding, Outcome};

use super::summarize_generation;
use crate::{
    cli::ReporterKind,
    error::CommandError,
    output,
    report_format::{
        DiagnosticBlock,
        format_dim,
        format_display,
        format_marker,
        format_painted,
        write_diagnostic_block,
    },
};

const fn format_reuse_tag(reused: bool, pin_reused: bool) -> &'static str {
    if reused {
        " \u{b7} cached"
    } else if pin_reused {
        " \u{b7} replayed pin"
    } else {
        ""
    }
}

fn write_finding(out: &mut impl Write, label: &str, row: &Finding) -> io::Result<()> {
    let title = format!("{}/{}", row.contract, row.check);
    let subject = row
        .subject
        .as_deref()
        .map(|path| path.display().to_string());
    if label == "PASS" {
        let location = subject
            .as_deref()
            .map(|path| format!("  {}", format_dim(&format_display(path))))
            .unwrap_or_default();
        let reuse = format_reuse_tag(row.reused, row.pin_reused == Some(true));
        let tag = if reuse.is_empty() {
            String::new()
        } else {
            format!(" {}", format_dim(reuse.trim_start()))
        };
        writeln!(
            out,
            "  {} {}  {}{location}{tag}",
            format_marker(label),
            format_painted(label),
            format_display(&title),
        )?;
        return writeln!(out, "        {}", format_dim(&format_display(&row.message)));
    }
    write_diagnostic_block(
        out,
        &DiagnosticBlock {
            help: None,
            label,
            location: subject.as_deref(),
            message: &row.message,
            meta: None,
            title: &title,
        },
    )
}

fn write_rows(out: &mut impl Write, rows: &[Finding]) -> io::Result<()> {
    for row in rows {
        let label = match row.outcome {
            Outcome::Passed => "PASS",
            Outcome::Failed => "FAIL",
            Outcome::Unresolved => "REVIEW",
        };
        write_finding(out, label, row)?;
    }
    Ok(())
}

pub(crate) fn write_section(out: &mut impl Write, rows: &[Finding]) -> io::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    writeln!(
        out,
        "\n {}",
        output::format_styled("Behavior checks").bold()
    )?;
    write_rows(out, rows)
}

struct PassedSummary {
    all_pin_reused: bool,
    all_reused: bool,
    count: usize,
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "Passing check counts are bounded by the rows returned in one run."
)]
fn write_default_rows(out: &mut impl Write, rows: &[Finding]) -> io::Result<()> {
    let mut passed_counts: BTreeMap<&str, PassedSummary> = BTreeMap::new();
    let mut wrote_diagnostic = false;
    for row in rows {
        let label = match row.outcome {
            Outcome::Passed => {
                let entry = passed_counts
                    .entry(row.contract.as_str())
                    .or_insert(PassedSummary {
                        all_pin_reused: true,
                        all_reused: true,
                        count: 0,
                    });
                entry.count += 1;
                entry.all_reused &= row.reused;
                entry.all_pin_reused &= row.pin_reused == Some(true);
                continue;
            },
            Outcome::Failed => "FAIL",
            Outcome::Unresolved => "REVIEW",
        };
        write_finding(out, label, row)?;
        wrote_diagnostic = true;
    }
    if wrote_diagnostic && !passed_counts.is_empty() {
        writeln!(out)?;
    }
    for (contract, summary) in passed_counts {
        let noun = if summary.count == 1 {
            "behavior check passed"
        } else {
            "behavior checks passed"
        };
        let tag = format_reuse_tag(summary.all_reused, summary.all_pin_reused);
        writeln!(
            out,
            "  {} {}  {}  {}",
            format_marker("PASS"),
            format_painted("PASS"),
            format_display(contract),
            format_dim(&format!("{} {noun}{tag}", summary.count))
        )?;
    }
    Ok(())
}

pub(crate) fn emit(rows: &[Finding], kind: ReporterKind) -> Result<(), CommandError> {
    match kind {
        ReporterKind::Jsonl => {
            output::write_document(
                &mut output::open_stdout().lock(),
                tenet_engine::EVENTS_VERSION,
                json!({
                    "type": "behavior_checks",
                    "checks": rows,
                    "generation": summarize_generation(rows)
                }),
            )?;
        },
        ReporterKind::Verbose => write_rows(&mut output::open_stderr().lock(), rows)?,
        ReporterKind::Default => write_default_rows(&mut output::open_stderr().lock(), rows)?,
    }
    Ok(())
}
