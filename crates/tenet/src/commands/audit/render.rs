use std::io::Write;

use serde::Serialize;
use serde_json::json;
use tenet_checks::behavior::{AuditModuleReport, Finding, Outcome};

use super::save::SavedContract;
use crate::{
    error::CommandError,
    output,
    report_format::{format_dim, format_display, format_marker},
};

const AUDIT_VERSION: u64 = 1;

struct RenderedAudit<'report> {
    deep: bool,
    guards: Vec<WitnessRow<'report>>,
    reports: &'report [AuditModuleReport],
    saved: &'report [SavedContract],
    total_cost: Option<f64>,
    total_requests: usize,
    unresolved: Vec<WitnessRow<'report>>,
    witnesses: Vec<WitnessRow<'report>>,
}

#[derive(Clone, Debug, Serialize)]
struct WitnessRow<'row> {
    check: &'row str,
    contract: &'row str,
    message: &'row str,
    module: &'row str,
    subject: Option<&'row str>,
}

/// Write either the JSON audit document or the concise terminal defect queue.
pub(super) fn write_report(
    out: &mut impl Write,
    deep: bool,
    reports: &[AuditModuleReport],
    saved: &[SavedContract],
    json_mode: bool,
) -> Result<(), CommandError> {
    let rendered = RenderedAudit {
        deep,
        guards: collect_rows(reports, Outcome::Passed),
        reports,
        saved,
        total_cost: sum_costs(reports),
        total_requests: reports.iter().map(|report| report.requests).sum(),
        unresolved: collect_rows(reports, Outcome::Unresolved),
        witnesses: collect_rows(reports, Outcome::Failed),
    };
    if json_mode {
        return write_json(out, &rendered);
    }
    write_terminal(out, &rendered)
}

fn collect_rows(reports: &[AuditModuleReport], target: Outcome) -> Vec<WitnessRow<'_>> {
    let mut rows = Vec::new();
    for report in reports {
        for finding in &report.findings {
            if finding.outcome == target {
                rows.push(format_witness_row(&report.name, finding));
            }
        }
    }
    rows
}

fn format_witness_row<'row>(module: &'row str, finding: &'row Finding) -> WitnessRow<'row> {
    WitnessRow {
        check: &finding.check,
        contract: &finding.contract,
        message: &finding.message,
        module,
        subject: finding.subject.as_deref().and_then(|path| path.to_str()),
    }
}

#[expect(
    clippy::float_arithmetic,
    reason = "Summing returned USD costs across audited modules is explicit financial accounting."
)]
fn sum_costs(reports: &[AuditModuleReport]) -> Option<f64> {
    let mut total = 0.0_f64;
    for report in reports {
        let cost = report.cost_usd?;
        total += cost;
    }
    Some(total + 0.0_f64)
}

fn write_json(out: &mut impl Write, rendered: &RenderedAudit<'_>) -> Result<(), CommandError> {
    output::write_document(
        out,
        AUDIT_VERSION,
        json!({
            "deep": rendered.deep,
            "modules": rendered.reports,
            "witnesses": rendered.witnesses,
            "guards": rendered.guards,
            "unresolved": rendered.unresolved,
            "saved": rendered.saved,
            "summary": {
                "modules": rendered.reports.len(),
                "witnesses": rendered.witnesses.len(),
                "guards": rendered.guards.len(),
                "unresolved": rendered.unresolved.len(),
                "saved": rendered.saved.len(),
                "requests": rendered.total_requests,
                "cost_usd": rendered.total_cost,
            },
        }),
    )?;
    Ok(())
}

fn write_saved_section(out: &mut impl Write, saved: &[SavedContract]) -> Result<(), CommandError> {
    if saved.is_empty() {
        return Ok(());
    }
    writeln!(out, "{}", output::format_styled("Saved contracts").bold())?;
    for item in saved {
        writeln!(
            out,
            "  {} {}  {}",
            format_marker("PASS"),
            item.name,
            format_dim(&item.path.display().to_string()),
        )?;
    }
    writeln!(out)?;
    Ok(())
}

fn write_terminal(out: &mut impl Write, rendered: &RenderedAudit<'_>) -> Result<(), CommandError> {
    let mode_label = if rendered.deep { "deep" } else { "standard" };
    writeln!(
        out,
        "{}  {}\n",
        output::format_styled("Tenet Audit").bold(),
        format_dim(&format!(
            "{mode_label} · {} modules",
            rendered.reports.len()
        )),
    )?;
    write_witness_section(out, &rendered.witnesses, &rendered.unresolved)?;
    write_saved_section(out, rendered.saved)?;
    writeln!(
        out,
        "  {}  {} failing · {} passing guards · {} unresolved · {} requests",
        output::format_styled("Summary").bold(),
        rendered.witnesses.len(),
        rendered.guards.len(),
        rendered.unresolved.len(),
        rendered.total_requests,
    )?;
    if rendered.saved.is_empty() && (!rendered.witnesses.is_empty() || !rendered.guards.is_empty())
    {
        writeln!(
            out,
            "  {} Run `tenet audit --save` to persist verified contracts and pinned checks into \
             `.contracts/`.",
            output::format_styled("= help:").cyan(),
        )?;
    }
    Ok(())
}

fn write_witness_section(
    out: &mut impl Write,
    witnesses: &[WitnessRow<'_>],
    unresolved: &[WitnessRow<'_>],
) -> Result<(), CommandError> {
    if witnesses.is_empty() && unresolved.is_empty() {
        writeln!(
            out,
            "  {} No verified behavioral defects found.",
            format_marker("PASS"),
        )?;
        return Ok(());
    }
    for witness in witnesses {
        let location = witness.subject.unwrap_or(witness.module);
        writeln!(
            out,
            "  {} {}/{}  {}",
            format_marker("FAIL"),
            output::format_styled(witness.contract).bold(),
            witness.check,
            format_dim(location),
        )?;
        writeln!(out, "    {}\n", format_display(witness.message))?;
    }
    if witnesses.is_empty() {
        for item in unresolved {
            let location = item.subject.unwrap_or(item.module);
            writeln!(
                out,
                "  {} {}/{}  {}",
                format_marker("REVIEW"),
                output::format_styled(item.contract).bold(),
                item.check,
                format_dim(location),
            )?;
            writeln!(out, "    {}\n", format_display(item.message))?;
        }
    }
    Ok(())
}
