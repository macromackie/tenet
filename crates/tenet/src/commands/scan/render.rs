mod terminal;

use alloc::collections::BTreeMap;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::json;
use tenet_adoption::{Added, Catalog};
use tenet_checks::behavior::{AuditModuleReport, Outcome};
use terminal::write_scan_terminal;

use super::{audit::SavedContract, init::serialize_added};
use crate::{cli::ExitStatus, error::CommandError, output};

const SCAN_VERSION: u64 = 1;

pub(super) struct ScanOutput<'output> {
    pub apply: bool,
    pub catalog: Option<&'output Catalog>,
    pub ephemeral: bool,
    pub installed: &'output [Added],
    pub notes: &'output [String],
    pub reports: &'output [AuditModuleReport],
    pub repro_paths: &'output BTreeMap<(String, String), PathBuf>,
    pub saved: &'output [SavedContract],
    pub scope: &'output Path,
    pub status: ExitStatus,
}

#[derive(Serialize)]
struct ScanWitness<'row> {
    check: &'row str,
    contract: &'row str,
    message: &'row str,
    module: &'row str,
    #[serde(skip_serializing_if = "Option::is_none")]
    repro_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    severity: Option<&'row str>,
    subject: Option<&'row str>,
}

pub(super) fn write_scan_output(
    out: &mut impl Write,
    payload: &ScanOutput<'_>,
    json_mode: bool,
) -> Result<(), CommandError> {
    if json_mode {
        return write_scan_json(out, payload);
    }
    write_scan_terminal(out, payload)
}

fn collect_witnesses<'output>(
    reports: &'output [AuditModuleReport],
    repro_paths: &BTreeMap<(String, String), PathBuf>,
) -> Vec<ScanWitness<'output>> {
    reports
        .iter()
        .flat_map(|report| {
            report
                .findings
                .iter()
                .filter(|finding| finding.outcome == Outcome::Failed)
                .map(move |finding| {
                    ScanWitness {
                        check: &finding.check,
                        contract: &finding.contract,
                        message: &finding.message,
                        module: &report.name,
                        repro_path: repro_paths
                            .get(&(finding.contract.clone(), finding.check.clone()))
                            .map(|path| path.display().to_string()),
                        severity: finding.read_effective_severity(),
                        subject: finding.subject.as_deref().and_then(Path::to_str),
                    }
                })
        })
        .collect()
}

fn count_synthesized(reports: &[AuditModuleReport]) -> usize {
    reports.iter().map(|report| report.contracts.len()).sum()
}

const fn label_mode(payload: &ScanOutput<'_>) -> &'static str {
    if payload.apply {
        "apply"
    } else if payload.ephemeral {
        "ephemeral"
    } else {
        "preview"
    }
}

fn write_scan_json(out: &mut impl Write, payload: &ScanOutput<'_>) -> Result<(), CommandError> {
    let witnesses = collect_witnesses(payload.reports, payload.repro_paths);
    let installed_entries = serialize_added(payload.installed);
    let catalog_json = payload.catalog.map(|catalog| {
        json!({
            "release": catalog.release,
            "commit": catalog.commit,
            "recommendations": catalog.candidates,
        })
    });
    output::write_document(
        out,
        SCAN_VERSION,
        json!({
            "mode": label_mode(payload),
            "applied": payload.apply,
            "ephemeral": payload.ephemeral,
            "catalog": catalog_json,
            "installed": installed_entries,
            "modules": payload.reports,
            "saved": payload.saved,
            "witnesses": witnesses,
            "notes": payload.notes,
            "exit_code": u8::from(payload.status),
        }),
    )?;
    Ok(())
}
