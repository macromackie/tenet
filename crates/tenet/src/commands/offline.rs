mod render;

use alloc::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use render::{OfflineOutput, write_offline_output};
use tenet_checks::{
    behavior::{
        AuditReplayRequest,
        AuditReplayResult,
        AuditReplayTarget,
        Outcome,
        PinnedCheck,
        find_pin_path,
        read_pin_summary,
        replay_audit,
    },
    fixes,
    mechanical::{self, Freshness, LocalMode},
};
use tenet_contracts::Contract;
use tenet_engine::{Selection, discover_contracts};

use super::audit;
use crate::{
    cli::{Check, ExitStatus, ReporterKind},
    error::CommandError,
    output,
    review::{OfflineReceipt, compute_coverage, discover_all_contracts},
};

/// One pinned behavior check discovered from committed `.contracts/` or `<state>/scan/.contracts/`.
pub(super) struct PinnedTargetEntry {
    pub contract: Contract,
    pub lock_path: String,
    pub target: AuditReplayTarget,
}

/// Build `runtime_paths` for sandboxed replay, guaranteeing every existing target subject is
/// included.
pub(super) fn build_replay_runtime_paths(
    root: &Path,
    targets: &[AuditReplayTarget],
) -> Result<Vec<String>, CommandError> {
    let mut paths = audit::discover_modules(root, &[], None)?.runtime_paths;
    let mut seen_paths: BTreeSet<String> = paths.iter().cloned().collect();
    for target in targets {
        if let Some(subject) = target.subject.as_deref()
            && !subject.is_empty()
            && root.join(subject).is_file()
            && seen_paths.insert(subject.to_owned())
        {
            paths.push(subject.to_owned());
        }
    }
    Ok(paths)
}

/// Discover pinned runtime checks across committed `.contracts/` and `<state>/scan/.contracts/`.
pub(super) fn collect_pinned_targets(
    root: &Path,
    state: &Path,
    contract_filter: Option<&str>,
    filters: &[PathBuf],
) -> Result<(Vec<PinnedTargetEntry>, Vec<AuditReplayResult>), CommandError> {
    let (committed, ephemeral) = discover_all_contracts(root, state)?;
    let scan_root = state.join("scan");
    let mut entries = Vec::new();
    let mut malformed = Vec::new();
    collect_scope_targets(
        root,
        root,
        "root",
        &committed,
        contract_filter,
        filters,
        &mut entries,
        &mut malformed,
    );
    collect_scope_targets(
        &scan_root,
        state,
        "state",
        &ephemeral,
        contract_filter,
        filters,
        &mut entries,
        &mut malformed,
    );
    Ok((entries, malformed))
}

/// Run deterministic static, local, and pinned runtime checks without semantic model review
/// (`tenet check --skip-semantic`, aliases `--offline`, `--replay`).
pub(super) async fn run(
    root: &Path,
    state: &Path,
    check: &Check,
) -> Result<ExitStatus, CommandError> {
    let json_mode = check.run.json || check.run.reporter == ReporterKind::Jsonl;
    let filters = audit::normalize_filters(&check.input.paths)?;
    let (native_report, local_rows) = run_static_and_local(root, state, check).await?;
    let (entries, mut behavior_results) =
        collect_pinned_targets(root, state, check.input.contract.as_deref(), &filters)?;
    if !entries.is_empty() {
        let targets: Vec<AuditReplayTarget> =
            entries.into_iter().map(|entry| entry.target).collect();
        let runtime_paths = build_replay_runtime_paths(root, &targets)?;
        let summary = format!("Replaying {} pinned checks", targets.len());
        let request = AuditReplayRequest {
            root: root.to_path_buf(),
            runtime_paths,
            state: state.to_path_buf(),
            targets,
        };
        let response = audit::run_with_progress(json_mode, &summary, |callback| {
            replay_audit(&request, callback)
                .context("run isolated offline replay worker")
                .map_err(CommandError::from)
        })?;
        behavior_results.extend(response.results);
    }
    let coverage = compute_coverage(root, state)?;
    let status = compute_offline_status(&native_report, &local_rows, &behavior_results);
    save_offline_receipt(state, &native_report, &local_rows, &behavior_results)?;
    let mut out = output::open_stdout().lock();
    let payload = OfflineOutput {
        behavior: &behavior_results,
        coverage: &coverage,
        local: &local_rows,
        native: &native_report,
        status,
    };
    write_offline_output(&mut out, &payload, json_mode)?;
    Ok(status)
}

fn save_offline_receipt(
    state: &Path,
    native: &fixes::Report,
    local: &[mechanical::Evidence],
    behavior: &[AuditReplayResult],
) -> Result<(), CommandError> {
    let native_passed = native.verified.iter().filter(|item| item.passed).count();
    let native_failed = native.verified.len().saturating_sub(native_passed);
    OfflineReceipt::new(behavior.to_vec(), local.len(), native_failed, native_passed).save(state)
}

#[expect(
    clippy::too_many_arguments,
    reason = "One helper walks both committed and ephemeral contract sets without duplicating \
              logic."
)]
fn collect_scope_targets(
    contract_base: &Path,
    pin_base: &Path,
    pin_root: &str,
    contracts: &[Contract],
    contract_filter: Option<&str>,
    filters: &[PathBuf],
    entries: &mut Vec<PinnedTargetEntry>,
    malformed: &mut Vec<AuditReplayResult>,
) {
    for contract in contracts {
        if contract_filter.is_some_and(|name| contract.name != name) {
            continue;
        }
        let contract_dir = contract.path.parent().map_or_else(
            || contract_base.to_path_buf(),
            |parent| contract_base.join(parent),
        );
        let Some(pin_path) = find_pin_path(&contract_dir) else {
            continue;
        };
        let summary = match read_pin_summary(&pin_path) {
            Ok(summary) => summary,
            Err(message) => {
                malformed.push(AuditReplayResult {
                    check: String::from("pin"),
                    contract: contract.name.clone(),
                    expected: None,
                    message,
                    observed: None,
                    outcome: Outcome::Unresolved,
                    subject: None,
                });
                continue;
            },
        };
        let lock_rel = pin_path.strip_prefix(pin_base).map_or_else(
            |_| audit::format_slash_path(&pin_path),
            audit::format_slash_path,
        );
        push_pinned_checks(
            contract,
            &lock_rel,
            pin_root,
            summary.checks,
            filters,
            entries,
        );
    }
}

fn push_pinned_checks(
    contract: &Contract,
    lock_rel: &str,
    pin_root: &str,
    checks: Vec<PinnedCheck>,
    filters: &[PathBuf],
    entries: &mut Vec<PinnedTargetEntry>,
) {
    for check in checks {
        if !filters.is_empty()
            && !check.subject.is_empty()
            && !audit::is_matching_filter(Path::new(&check.subject), filters)
        {
            continue;
        }
        let subject = (!check.subject.is_empty()).then_some(check.subject);
        entries.push(PinnedTargetEntry {
            contract: contract.clone(),
            lock_path: lock_rel.to_owned(),
            target: AuditReplayTarget {
                check: check.id,
                contract: contract.name.clone(),
                lock_path: Some(lock_rel.to_owned()),
                pin_root: Some(pin_root.to_owned()),
                repro: None,
                subject,
            },
        });
    }
}

fn compute_offline_status(
    native: &fixes::Report,
    local: &[mechanical::Evidence],
    behavior: &[AuditReplayResult],
) -> ExitStatus {
    let native_failed = native.verified.iter().any(|item| !item.passed);
    let local_failed = local.iter().any(|row| {
        row.freshness == Freshness::Current
            && row.observed_outcome == Some(mechanical::Outcome::Fail)
    });
    let behavior_failed = behavior.iter().any(|row| row.outcome == Outcome::Failed);
    if native_failed || local_failed || behavior_failed {
        return ExitStatus::Failed;
    }
    let native_unresolved = !native.unresolved.is_empty();
    let local_unresolved = local.iter().any(|row| {
        row.freshness != Freshness::Current
            || row.observed_outcome != Some(mechanical::Outcome::Pass)
    });
    let behavior_unresolved = behavior
        .iter()
        .any(|row| row.outcome == Outcome::Unresolved);
    if native_unresolved || local_unresolved || behavior_unresolved {
        return ExitStatus::Unresolved;
    }
    ExitStatus::Passed
}

async fn run_static_and_local(
    root: &Path,
    state: &Path,
    check: &Check,
) -> Result<(fixes::Report, Vec<mechanical::Evidence>), CommandError> {
    let committed = discover_contracts(root)?;
    if committed.is_empty() {
        return Ok((fixes::Report::default(), Vec::new()));
    }
    let selection = Selection {
        base: check.input.base.clone(),
        contract: check.input.contract.clone(),
        paths: check.input.paths.clone(),
    };
    let plan = tenet_engine::plan(root, &selection)?;
    let native_report = fixes::inspect(&plan).await.map_err(anyhow::Error::from)?;
    let has_obligations = plan
        .contracts
        .iter()
        .any(|contract| !contract.obligations.is_empty());
    let local_rows = if has_obligations {
        let mode = if check.fresh {
            LocalMode::Fresh
        } else {
            LocalMode::Reuse
        };
        mechanical::run_selected_local(&plan, state, None, mode).await?
    } else {
        Vec::new()
    };
    Ok((native_report, local_rows))
}
