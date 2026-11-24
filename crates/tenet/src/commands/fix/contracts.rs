mod groups;

use alloc::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Context as _;
use groups::build_groups_from_replay;
use tenet_checks::behavior::{
    AuditContractOutput,
    AuditFixGroup,
    AuditReplayRequest,
    AuditReplayTarget,
    Outcome,
    replay_audit,
};

use super::super::{
    audit::{
        AuditQueue,
        build_contract_lock_paths,
        build_replay_target,
        discover_modules,
        format_slash_path,
        normalize_filters,
        run_with_progress,
    },
    offline::{PinnedTargetEntry, build_replay_runtime_paths, collect_pinned_targets},
};
use crate::{cli::Fix, error::CommandError};

const REPLAYABLE_OUTCOMES: &[Outcome] = &[Outcome::Failed, Outcome::Passed];

/// Discovered live failing runtime repair groups plus runtime source closure.
pub(super) struct RuntimeFixPlan {
    pub groups: Vec<AuditFixGroup>,
    pub runtime_paths: Vec<String>,
}

struct ReplayedTarget {
    contract_doc: AuditContractOutput,
    severity: Option<String>,
    target: AuditReplayTarget,
}

/// Replay pinned checks from committed `.contracts/`, ephemeral `<state>/scan/.contracts/`, and
/// legacy `<state>/audit.json` to partition live failing repros from passing regression guards.
pub(super) fn prepare_runtime_fix_plan(
    root: &Path,
    state: &Path,
    args: &Fix,
) -> Result<RuntimeFixPlan, CommandError> {
    let filters = normalize_filters(&args.paths)?;
    let (pinned_entries, _) = collect_pinned_targets(root, state, None, &[])?;
    let mut tracked = collect_tracked_from_pins(pinned_entries);
    append_legacy_audit_targets(state, &mut tracked);
    if tracked.is_empty() {
        return Ok(RuntimeFixPlan {
            groups: Vec::new(),
            runtime_paths: Vec::new(),
        });
    }
    let targets: Vec<AuditReplayTarget> =
        tracked.iter().map(|entry| entry.target.clone()).collect();
    let runtime_paths = build_replay_runtime_paths(root, &targets)?;
    let summary = format!("Replaying {} pinned checks", targets.len());
    let request = AuditReplayRequest {
        root: root.to_path_buf(),
        runtime_paths: runtime_paths.clone(),
        state: state.to_path_buf(),
        targets,
    };
    let response = run_with_progress(args.json, &summary, |callback| {
        replay_audit(&request, callback)
            .context("run offline replay before fix")
            .map_err(CommandError::from)
    })?;
    let discovered = discover_modules(root, &[], None)?;
    let groups = build_groups_from_replay(
        root,
        &tracked,
        &response.results,
        args.contract.as_deref(),
        &filters,
        args.severity.to_level(),
        &discovered.modules,
        &runtime_paths,
    );
    Ok(RuntimeFixPlan {
        groups,
        runtime_paths,
    })
}

fn append_legacy_audit_targets(state: &Path, tracked: &mut Vec<ReplayedTarget>) {
    let Ok(queue) = AuditQueue::read_existing(state) else {
        return;
    };
    update_pinned_severities(&queue, tracked);
    let keys = collect_existing_keys(tracked);
    for report in queue.modules.values() {
        let lock_paths = build_contract_lock_paths(&report.contracts);
        let named_contracts: BTreeMap<&str, &AuditContractOutput> = report
            .contracts
            .iter()
            .map(|contract| (contract.name.as_str(), contract))
            .collect();
        for finding in &report.findings {
            if !REPLAYABLE_OUTCOMES.contains(&finding.outcome) {
                continue;
            }
            let key = (finding.contract.clone(), finding.check.clone());
            let subject_key = finding
                .subject
                .as_deref()
                .map(|subject| (finding.check.clone(), format_slash_path(subject)));
            if keys.exact_keys.contains(&key)
                || subject_key
                    .as_ref()
                    .is_some_and(|pair| keys.check_subjects.contains(pair))
            {
                continue;
            }
            let Some(contract_doc) = named_contracts.get(finding.contract.as_str()) else {
                continue;
            };
            let mut target = build_replay_target(finding, &lock_paths);
            target.pin_root = Some(String::from("state"));
            tracked.push(ReplayedTarget {
                contract_doc: (*contract_doc).clone(),
                severity: finding.read_effective_severity().map(str::to_owned),
                target,
            });
        }
    }
}

struct TrackedKeys {
    check_subjects: BTreeSet<(String, String)>,
    exact_keys: BTreeSet<(String, String)>,
}

fn collect_existing_keys(tracked: &[ReplayedTarget]) -> TrackedKeys {
    let exact_keys = tracked
        .iter()
        .map(|item| (item.target.contract.clone(), item.target.check.clone()))
        .collect();
    let check_subjects = tracked
        .iter()
        .filter_map(|item| {
            item.target
                .subject
                .as_ref()
                .map(|subject| (item.target.check.clone(), subject.clone()))
        })
        .collect();
    TrackedKeys {
        check_subjects,
        exact_keys,
    }
}

fn update_pinned_severities(queue: &AuditQueue, tracked: &mut [ReplayedTarget]) {
    let mut exact_severities = BTreeMap::new();
    let mut subject_severities = BTreeMap::new();
    for report in queue.modules.values() {
        for finding in &report.findings {
            let Some(severity) = finding.read_effective_severity() else {
                continue;
            };
            let _prior = exact_severities.insert(
                (finding.contract.clone(), finding.check.clone()),
                severity.to_owned(),
            );
            if let Some(subject) = finding.subject.as_deref() {
                let _prior_subject = subject_severities.insert(
                    (finding.check.clone(), format_slash_path(subject)),
                    severity.to_owned(),
                );
            }
        }
    }
    for item in tracked {
        if item.severity.is_some() {
            continue;
        }
        let exact_key = (item.target.contract.clone(), item.target.check.clone());
        if let Some(severity) = exact_severities.get(&exact_key) {
            item.severity = Some(severity.clone());
            continue;
        }
        if let Some(subject) = &item.target.subject
            && let Some(severity) =
                subject_severities.get(&(item.target.check.clone(), subject.clone()))
        {
            item.severity = Some(severity.clone());
        }
    }
}

fn collect_tracked_from_pins(entries: Vec<PinnedTargetEntry>) -> Vec<ReplayedTarget> {
    entries
        .into_iter()
        .map(|entry| {
            let paths = entry
                .target
                .subject
                .iter()
                .cloned()
                .collect::<Vec<String>>();
            let contract_doc = AuditContractOutput {
                document: entry.contract.document,
                lock_path: entry.lock_path,
                message: entry.contract.message,
                name: entry.contract.name,
                paths,
            };
            ReplayedTarget {
                contract_doc,
                severity: None,
                target: entry.target,
            }
        })
        .collect()
}
