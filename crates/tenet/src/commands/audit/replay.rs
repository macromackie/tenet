use alloc::collections::BTreeMap;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use serde_json::json;
use tenet_checks::behavior::{
    AuditContractOutput,
    AuditReplayRequest,
    AuditReplayResult,
    AuditReplayTarget,
    Finding,
    Outcome,
    replay_audit,
};

use super::{
    discover::{discover_modules, format_slash_path, is_matching_filter, normalize_filters},
    queue::AuditQueue,
    run_with_progress,
};
use crate::{
    cli::{Audit, ExitStatus},
    error::CommandError,
    output,
    report_format::{format_dim, format_display, format_marker, format_painted},
};

const REPLAY_VERSION: u64 = 1;

/// Display fields for one rendered replay or repair witness row.
pub(in crate::commands) struct StatusRow<'row> {
    pub check: &'row str,
    pub contract: &'row str,
    pub expected: Option<&'row str>,
    pub label: String,
    pub marker: String,
    pub message: &'row str,
    pub observed: Option<&'row str>,
    pub subject: Option<&'row str>,
}

/// Build a lookup from contract name to its staged pin path within one audit module report.
pub(in crate::commands) fn build_contract_lock_paths(
    contracts: &[AuditContractOutput],
) -> BTreeMap<&str, &str> {
    contracts
        .iter()
        .map(|contract| (contract.name.as_str(), contract.lock_path.as_str()))
        .collect()
}

/// Convert a saved audit finding into a worker replay target.
pub(in crate::commands) fn build_replay_target(
    finding: &Finding,
    contract_lock_paths: &BTreeMap<&str, &str>,
) -> AuditReplayTarget {
    AuditReplayTarget {
        check: finding.check.clone(),
        contract: finding.contract.clone(),
        lock_path: contract_lock_paths
            .get(finding.contract.as_str())
            .map(|path| (*path).to_owned()),
        pin_root: None,
        repro: finding.repro.clone(),
        subject: finding.subject.as_deref().map(format_slash_path),
    }
}

/// Return whether a finding in `module_name` matches the normalized user path filters.
pub(in crate::commands) fn is_matching_finding(
    module_name: &str,
    subject: Option<&Path>,
    filters: &[PathBuf],
) -> bool {
    if filters.is_empty() {
        return true;
    }
    if let Some(path) = subject {
        return is_matching_filter(path, filters);
    }
    let base_module = module_name
        .split_once('#')
        .map_or(module_name, |(prefix, _)| prefix);
    is_matching_filter(Path::new(base_module), filters)
}

/// Replay saved failing audit witnesses against the current checkout without calling a model.
pub(super) fn run(root: &Path, state: &Path, args: &Audit) -> Result<ExitStatus, CommandError> {
    let queue = AuditQueue::read_existing(state)?;
    let filters = normalize_filters(&args.paths)?;
    let targets = collect_replay_targets(&queue, &filters);
    let results = if targets.is_empty() {
        Vec::new()
    } else {
        let runtime_paths = discover_modules(root, &[], None)?.runtime_paths;
        let summary = format!("Replaying {} witnesses", targets.len());
        let request = AuditReplayRequest {
            root: root.to_path_buf(),
            runtime_paths,
            state: state.to_path_buf(),
            targets,
        };
        let response = run_with_progress(args.json, &summary, |callback| {
            replay_audit(&request, callback)
                .context("run isolated audit replay worker")
                .map_err(CommandError::from)
        })?;
        response.results
    };
    let status = read_replay_exit_status(&results);
    let mut out = output::open_stdout().lock();
    if args.json {
        write_replay_json(&mut out, &results, status)?;
    } else {
        write_replay_terminal(&mut out, &results)?;
    }
    Ok(status)
}

/// Write one formatted witness outcome row to `out`.
pub(in crate::commands) fn write_status_row(
    out: &mut impl Write,
    row: &StatusRow<'_>,
) -> Result<(), CommandError> {
    let location = row.subject.unwrap_or("-");
    writeln!(
        out,
        "  {} {}  {}/{}  {}",
        row.marker,
        row.label,
        output::format_styled(row.contract).bold(),
        row.check,
        format_dim(location),
    )?;
    writeln!(out, "    {}", format_display(row.message))?;
    if let Some(detail) = format_expected_observed(row.expected, row.observed) {
        writeln!(out, "    {}", format_dim(&detail))?;
    }
    writeln!(out)?;
    Ok(())
}

fn collect_replay_targets(queue: &AuditQueue, filters: &[PathBuf]) -> Vec<AuditReplayTarget> {
    let mut targets = Vec::new();
    for report in queue.modules.values() {
        let contract_lock_paths = build_contract_lock_paths(&report.contracts);
        for finding in &report.findings {
            if finding.outcome != Outcome::Failed {
                continue;
            }
            if !is_matching_finding(&report.name, finding.subject.as_deref(), filters) {
                continue;
            }
            targets.push(build_replay_target(finding, &contract_lock_paths));
        }
    }
    targets
}

fn count_outcomes(results: &[AuditReplayResult]) -> (usize, usize, usize) {
    let mut fixed = 0_usize;
    let mut failed = 0_usize;
    let mut unresolved = 0_usize;
    for result in results {
        match result.outcome {
            Outcome::Passed => fixed = fixed.saturating_add(1),
            Outcome::Failed => failed = failed.saturating_add(1),
            Outcome::Unresolved => unresolved = unresolved.saturating_add(1),
        }
    }
    (fixed, failed, unresolved)
}

fn format_expected_observed(expected: Option<&str>, observed: Option<&str>) -> Option<String> {
    match (expected, observed) {
        (Some(exp), Some(obs)) => {
            Some(format!(
                "expected {} · got {}",
                format_display(exp),
                format_display(obs),
            ))
        },
        (Some(exp), None) => Some(format!("expected {}", format_display(exp))),
        (None, Some(obs)) => Some(format!("got {}", format_display(obs))),
        (None, None) => None,
    }
}

fn format_replay_badge(outcome: Outcome, pass_label: &str) -> (String, String) {
    match outcome {
        Outcome::Passed => {
            (
                format_marker("PASS"),
                output::format_styled(pass_label).green().bold().to_string(),
            )
        },
        Outcome::Failed => (format_marker("FAIL"), format_painted("FAIL")),
        Outcome::Unresolved => (format_marker("REVIEW"), format_painted("REVIEW")),
    }
}

/// Write a slice of replay results as formatted status rows using `pass_label` for passing rows.
pub(in crate::commands) fn write_replay_results(
    out: &mut impl Write,
    results: &[AuditReplayResult],
    pass_label: &str,
) -> Result<(), CommandError> {
    for result in results {
        let (marker, label) = format_replay_badge(result.outcome, pass_label);
        write_status_row(
            out,
            &StatusRow {
                check: &result.check,
                contract: &result.contract,
                expected: result.expected.as_deref(),
                label,
                marker,
                message: &result.message,
                observed: result.observed.as_deref(),
                subject: result.subject.as_deref(),
            },
        )?;
    }
    Ok(())
}

fn read_replay_exit_status(results: &[AuditReplayResult]) -> ExitStatus {
    if results.iter().any(|row| row.outcome == Outcome::Failed) {
        return ExitStatus::Failed;
    }
    if results.iter().any(|row| row.outcome == Outcome::Unresolved) {
        return ExitStatus::Unresolved;
    }
    ExitStatus::Passed
}

fn write_replay_json(
    out: &mut impl Write,
    results: &[AuditReplayResult],
    status: ExitStatus,
) -> Result<(), CommandError> {
    let (fixed, failed, unresolved) = count_outcomes(results);
    output::write_document(
        out,
        REPLAY_VERSION,
        json!({
            "mode": "replay",
            "results": results,
            "summary": {
                "fixed": fixed,
                "failed": failed,
                "unresolved": unresolved,
                "requests": 0_usize,
            },
            "exit_code": u8::from(status),
        }),
    )?;
    Ok(())
}

fn write_replay_terminal(
    out: &mut impl Write,
    results: &[AuditReplayResult],
) -> Result<(), CommandError> {
    let (fixed, failed, unresolved) = count_outcomes(results);
    writeln!(
        out,
        "{}  {}\n",
        output::format_styled("Tenet Audit Replay").bold(),
        format_dim(&format!(
            "{} witnesses · zero model requests",
            results.len()
        )),
    )?;
    if results.is_empty() {
        writeln!(
            out,
            "  {} No failing witnesses to replay.\n",
            format_marker("PASS"),
        )?;
    } else {
        write_replay_results(out, results, "FIXED (PASS)")?;
    }
    writeln!(
        out,
        "  {}  {fixed} fixed · {failed} still failing · {unresolved} unresolved · 0 requests",
        output::format_styled("Summary").bold(),
    )?;
    Ok(())
}
