use alloc::collections::BTreeSet;
use std::io;

use serde_json::{Value, json};
use tenet_checks::{
    behavior::{Finding, Outcome as BehaviorOutcome},
    mechanical::{self, Freshness, Outcome},
};
use tenet_engine::review::{self, CurrentContract};

use super::{
    coverage::{RepoCoverage, write_coverage_section},
    offline::{OfflineReceipt, write_offline_next_step, write_offline_section},
};
use crate::{
    cli::ExitStatus,
    error::CommandError,
    output,
    report_format::{format_dim, format_display, format_marker, format_painted},
    review::{STATUS_VERSION, find_model, format_state_text},
};

/// Optional supplemental status sections shared by checked and unchecked status renderers.
pub(super) struct StatusExtras<'extras> {
    pub coverage: Option<&'extras RepoCoverage>,
    pub mechanical: Option<&'extras [mechanical::Evidence]>,
    pub offline: Option<&'extras OfflineReceipt>,
}

pub(super) fn collect_failed_behavior_contracts(rows: &[Finding]) -> BTreeSet<&str> {
    rows.iter()
        .filter(|row| row.outcome == BehaviorOutcome::Failed)
        .map(|row| row.contract.as_str())
        .collect()
}

pub(super) fn format_contract_row_state(
    entry: &CurrentContract<'_>,
    behavior_failed_contracts: &BTreeSet<&str>,
) -> (&'static str, String) {
    if !behavior_failed_contracts.contains(entry.contract.name.as_str()) {
        return format_state_text(entry);
    }
    let note = match find_model(entry).and_then(|verdict| verdict.violation) {
        Some(probability) => {
            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::float_arithmetic,
                reason = "Model violation probability is bounded in [0.0, 1.0] before percentage \
                          formatting."
            )]
            let percent = (probability.clamp(0.0_f64, 1.0_f64) * 100.0_f64).round() as u8;
            format!("behavior failed \u{b7} model {percent}%")
        },
        None => "behavior failed".into(),
    };
    ("FAIL", note)
}

const fn label_freshness(freshness: Freshness) -> &'static str {
    match freshness {
        Freshness::Current => "current",
        Freshness::Missing => "missing",
        Freshness::Stale => "stale",
        Freshness::Unresolved => "unresolved",
    }
}

const fn label_outcome(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Fail => "FAIL",
        Outcome::Pass => "PASS",
        Outcome::Unresolved => "REVIEW",
    }
}

const fn resolve_marker_label(row: &mechanical::Evidence) -> &'static str {
    match (row.freshness, row.observed_outcome) {
        (Freshness::Current, Some(Outcome::Pass)) => "PASS",
        (Freshness::Current, Some(Outcome::Fail)) => "FAIL",
        _ => "REVIEW",
    }
}

pub(crate) fn write_mechanical(
    out: &mut impl io::Write,
    rows: &[mechanical::Evidence],
) -> Result<(), CommandError> {
    if rows.is_empty() {
        return Ok(());
    }
    writeln!(out, "\n {}", output::format_styled("Local checks").bold())?;
    for row in rows {
        let marker_label = resolve_marker_label(row);
        write!(
            out,
            "  {} {}/{}  {}",
            format_marker(marker_label),
            format_display(&row.contract),
            format_display(row.name.as_str()),
            format_dim(label_freshness(row.freshness)),
        )?;
        if let Some(outcome) = row.observed_outcome {
            write!(out, " \u{b7} {}", format_painted(label_outcome(outcome)))?;
        }
        if let Some(checker) = &row.checker {
            write!(
                out,
                " {}",
                format_dim(&format!("by {}", format_display(checker)))
            )?;
        }
        if row.replay.is_none()
            && let Some(message) = &row.message
        {
            write!(out, " \u{b7} {}", format_display(message))?;
        }
        if let Some(reason) = &row.reason {
            write!(out, " \u{b7} {}", format_display(reason))?;
        }
        if row.reused == Some(true) {
            write!(out, " {}", format_dim("(reused)"))?;
        }
        writeln!(out)?;
        if let Some(replay) = &row.replay {
            write_details(out, row)?;
            writeln!(
                out,
                "      {}    {}",
                format_dim("Replay"),
                format_display(replay)
            )?;
        }
    }
    Ok(())
}

fn write_details(out: &mut impl io::Write, row: &mechanical::Evidence) -> io::Result<()> {
    if let Some(message) = &row.message {
        for line in message.lines() {
            writeln!(out, "      {}", format_display(line))?;
        }
    }
    if let Some(finding) = &row.finding {
        writeln!(
            out,
            "      {}      {}",
            format_dim("Case"),
            format_display(&finding.case)
        )?;
        writeln!(
            out,
            "      {}  {}",
            format_dim("Expected"),
            format_display(&finding.expected)
        )?;
        writeln!(
            out,
            "      {}    {}",
            format_dim("Actual"),
            format_display(&finding.actual)
        )?;
        if let Some(location) = &finding.location {
            writeln!(
                out,
                "      {}  {}",
                format_dim("Location"),
                format_display(location)
            )?;
        }
    }
    Ok(())
}

fn attach_optional_sections(document: &mut Value, extras: &StatusExtras<'_>) {
    if let Some(rows) = extras.mechanical {
        document["mechanical_evidence"] = json!(rows);
    }
    if let Some(map) = extras.coverage {
        document["coverage"] = json!(map);
    }
    if let Some(receipt) = extras.offline {
        document["offline_checks"] = json!(receipt);
    }
}

#[expect(
    clippy::indexing_slicing,
    reason = "The JSON document is constructed as an object before optional fields are inserted."
)]
pub(super) fn write_status_json(
    out: &mut impl io::Write,
    current: &review::Current<'_>,
    behavior_failed_contracts: &BTreeSet<&str>,
    generated: &[Finding],
    extras: &StatusExtras<'_>,
    status: ExitStatus,
) -> Result<(), CommandError> {
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
            value["behavior_failed"] =
                json!(behavior_failed_contracts.contains(entry.contract.name.as_str()));
            Ok(value)
        })
        .collect::<Result<Vec<_>, CommandError>>()?;
    let mut document = json!({
        "review": current.latest.review,
        "base": current.latest.base_oid,
        "revision": current.latest.rev,
        "unchanged": current.unchanged,
        "contracts": contracts,
        "deleted_contracts": current.deleted,
        "exit_code": u8::from(status),
    });
    document["behavior_checks"] = json!(generated);
    attach_optional_sections(&mut document, extras);
    output::write_document(out, STATUS_VERSION, document)?;
    Ok(())
}

pub(super) fn write_unchecked(
    out: &mut impl io::Write,
    base: Option<&str>,
    message: &str,
    extras: &StatusExtras<'_>,
    json: bool,
) -> Result<ExitStatus, CommandError> {
    let status = extras
        .offline
        .map_or(ExitStatus::Pending, OfflineReceipt::exit_status);
    if json {
        let mut document = json!({
            "base": base,
            "revision": null,
            "contracts": [],
            "exit_code": u8::from(status),
            "message": message,
        });
        attach_optional_sections(&mut document, extras);
        output::write_document(out, STATUS_VERSION, document)?;
        return Ok(status);
    }
    if let Some(receipt) = extras.offline {
        write_offline_section(out, receipt)?;
    } else {
        writeln!(out, "{message}")?;
    }
    if let Some(map) = extras.coverage {
        write_coverage_section(out, map)?;
    }
    if let Some(rows) = extras.mechanical {
        write_mechanical(out, rows)?;
    }
    if let Some(receipt) = extras.offline {
        write_offline_next_step(out, receipt)?;
    }
    Ok(status)
}
