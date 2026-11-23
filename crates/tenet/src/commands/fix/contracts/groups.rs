use alloc::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tenet_checks::behavior::{
    AuditFixGroup,
    AuditFixWitness,
    AuditModuleInput,
    AuditReplayResult,
    AuditReplayTarget,
    Outcome,
    SeverityLevel,
    is_severity_at_least,
};

use super::{
    super::{
        super::audit::{find_related_files, format_slash_path, is_matching_filter},
        audit::select_group_test_files,
    },
    ReplayedTarget,
};

#[expect(
    clippy::too_many_arguments,
    reason = "Grouping replay results filters failing repros while preserving passing guards."
)]
pub(super) fn build_groups_from_replay(
    root: &Path,
    tracked: &[ReplayedTarget],
    results: &[AuditReplayResult],
    contract_filter: Option<&str>,
    filters: &[PathBuf],
    min_severity: SeverityLevel,
    discovered_modules: &[AuditModuleInput],
    runtime_paths: &[String],
) -> Vec<AuditFixGroup> {
    let mut guards = Vec::new();
    let mut grouped_failures: BTreeMap<String, Vec<(&ReplayedTarget, &AuditReplayResult)>> =
        BTreeMap::new();
    for (entry, result) in tracked.iter().zip(results) {
        if result.outcome == Outcome::Passed {
            guards.push(entry.target.clone());
            continue;
        }
        if result.outcome != Outcome::Failed {
            continue;
        }
        if !is_matching_failing_filters(entry, result, contract_filter, filters, min_severity) {
            continue;
        }
        grouped_failures
            .entry(entry.target.contract.clone())
            .or_default()
            .push((entry, result));
    }
    grouped_failures
        .into_iter()
        .map(|(contract_name, items)| {
            build_single_group(
                root,
                &contract_name,
                &items,
                &guards,
                discovered_modules,
                runtime_paths,
            )
        })
        .collect()
}

fn build_single_group(
    root: &Path,
    contract_name: &str,
    items: &[(&ReplayedTarget, &AuditReplayResult)],
    all_guards: &[AuditReplayTarget],
    discovered_modules: &[AuditModuleInput],
    runtime_paths: &[String],
) -> AuditFixGroup {
    let mut subject_files = BTreeSet::new();
    let mut witnesses = Vec::with_capacity(items.len());
    let mut contracts = Vec::new();
    for (entry, result) in items {
        if contracts.is_empty() {
            contracts.push(entry.contract_doc.clone());
        }
        let subject = result
            .subject
            .clone()
            .or_else(|| entry.target.subject.clone());
        if let Some(path) = &subject {
            let _inserted = subject_files.insert(path.clone());
        }
        witnesses.push(AuditFixWitness {
            check: entry.target.check.clone(),
            contract: entry.target.contract.clone(),
            lock_path: entry.target.lock_path.clone(),
            message: result.message.clone(),
            pin_root: entry.target.pin_root.clone(),
            repro: entry.target.repro.clone(),
            subject,
        });
    }
    let guards = select_group_guards(contract_name, &subject_files, all_guards);
    let files: Vec<String> = subject_files.into_iter().collect();
    let related_files = find_related_files(root, &files, runtime_paths);
    let test_files = select_group_test_files(contract_name, &files, discovered_modules);
    AuditFixGroup {
        contracts,
        files,
        guards,
        name: contract_name.to_owned(),
        related_files,
        test_files,
        witnesses,
    }
}

fn extract_message_severity(message: &str) -> Option<&'static str> {
    const TAGS: [(&str, &str); 3] = [
        ("[critical]", "critical"),
        ("[high]", "high"),
        ("[medium]", "medium"),
    ];
    let prefix = message.split_whitespace().next()?;
    TAGS.iter()
        .find_map(|(tag, level)| prefix.eq_ignore_ascii_case(tag).then_some(*level))
}

fn is_matching_failing_filters(
    entry: &ReplayedTarget,
    result: &AuditReplayResult,
    contract_filter: Option<&str>,
    filters: &[PathBuf],
    min_severity: SeverityLevel,
) -> bool {
    if contract_filter.is_some_and(|name| entry.target.contract != name) {
        return false;
    }
    let severity = entry
        .severity
        .as_deref()
        .or_else(|| extract_message_severity(&result.message));
    if !is_severity_at_least(severity, min_severity) {
        return false;
    }
    if filters.is_empty() {
        return true;
    }
    result
        .subject
        .as_deref()
        .or(entry.target.subject.as_deref())
        .is_some_and(|path| {
            is_matching_filter(Path::new(&format_slash_path(Path::new(path))), filters)
        })
}

fn select_group_guards(
    contract_name: &str,
    subject_files: &BTreeSet<String>,
    all_guards: &[AuditReplayTarget],
) -> Vec<AuditReplayTarget> {
    all_guards
        .iter()
        .filter(|guard| {
            guard.contract == contract_name
                || guard
                    .subject
                    .as_deref()
                    .is_some_and(|path| subject_files.contains(path))
        })
        .cloned()
        .collect()
}
