use alloc::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tenet_checks::behavior::{
    AuditContractOutput,
    AuditFixGroup,
    AuditFixWitness,
    AuditModuleInput,
    AuditModuleReport,
    AuditReplayTarget,
    Outcome,
};

use super::super::super::audit::{
    AuditQueue,
    build_contract_lock_paths,
    build_replay_target,
    find_related_files,
    format_slash_path,
    is_matching_finding,
};

const MAX_GROUP_TEST_FILES: usize = 4;

/// Build filtered repair groups from a saved audit queue and discovered repository modules.
pub(super) fn build_fix_groups(
    root: &Path,
    queue: &AuditQueue,
    contract_filter: Option<&str>,
    filters: &[PathBuf],
    discovered_modules: &[AuditModuleInput],
    runtime_paths: &[String],
) -> Vec<AuditFixGroup> {
    let mut groups = Vec::new();
    for report in queue.modules.values() {
        if let Some(group) = build_module_group(
            root,
            report,
            contract_filter,
            filters,
            discovered_modules,
            runtime_paths,
        ) {
            groups.push(group);
        }
    }
    groups
}

/// Build one module repair group when `report` has matching failing witnesses.
pub(super) fn build_module_group(
    root: &Path,
    report: &AuditModuleReport,
    contract_filter: Option<&str>,
    filters: &[PathBuf],
    discovered_modules: &[AuditModuleInput],
    runtime_paths: &[String],
) -> Option<AuditFixGroup> {
    let contract_lock_paths = build_contract_lock_paths(&report.contracts);
    let (witnesses, selected_contract_names, mut selected_file_paths) =
        collect_group_witnesses(report, contract_filter, filters, &contract_lock_paths);
    if witnesses.is_empty() {
        return None;
    }
    let contracts = select_group_contracts(
        root,
        report,
        &selected_contract_names,
        filters,
        &mut selected_file_paths,
    );
    if selected_file_paths.is_empty()
        && let Some(module) = discovered_modules
            .iter()
            .find(|item| item.name == report.name)
    {
        for file in &module.files {
            let _inserted = selected_file_paths.insert(file.clone());
        }
    }
    let guards = collect_group_guards(
        report,
        &selected_contract_names,
        &selected_file_paths,
        &contract_lock_paths,
    );
    let files: Vec<String> = selected_file_paths.into_iter().collect();
    let related_files = find_related_files(root, &files, runtime_paths);
    let test_files = select_group_test_files(&report.name, &files, discovered_modules);
    Some(AuditFixGroup {
        contracts,
        files,
        guards,
        name: report.name.clone(),
        related_files,
        test_files,
        witnesses,
    })
}

fn collect_group_guards(
    report: &AuditModuleReport,
    selected_contract_names: &BTreeSet<&str>,
    selected_file_paths: &BTreeSet<String>,
    contract_lock_paths: &BTreeMap<&str, &str>,
) -> Vec<AuditReplayTarget> {
    let mut guards = Vec::new();
    for finding in &report.findings {
        if finding.outcome != Outcome::Passed {
            continue;
        }
        let matches_contract = selected_contract_names.contains(finding.contract.as_str());
        let matches_file = finding
            .subject
            .as_deref()
            .is_some_and(|path| selected_file_paths.contains(&format_slash_path(path)));
        if matches_contract || matches_file {
            guards.push(build_replay_target(finding, contract_lock_paths));
        }
    }
    guards
}

fn collect_group_witnesses<'report>(
    report: &'report AuditModuleReport,
    contract_filter: Option<&str>,
    filters: &[PathBuf],
    contract_lock_paths: &BTreeMap<&str, &str>,
) -> (
    Vec<AuditFixWitness>,
    BTreeSet<&'report str>,
    BTreeSet<String>,
) {
    let mut witnesses = Vec::new();
    let mut selected_contract_names = BTreeSet::new();
    let mut selected_file_paths = BTreeSet::new();
    for finding in &report.findings {
        if finding.outcome != Outcome::Failed
            || contract_filter.is_some_and(|name| finding.contract != name)
            || !is_matching_finding(&report.name, finding.subject.as_deref(), filters)
        {
            continue;
        }
        let target = build_replay_target(finding, contract_lock_paths);
        if let Some(path) = &target.subject {
            let _inserted = selected_file_paths.insert(path.clone());
        }
        let _inserted = selected_contract_names.insert(finding.contract.as_str());
        witnesses.push(AuditFixWitness {
            check: target.check,
            contract: target.contract,
            lock_path: target.lock_path,
            message: finding.message.clone(),
            repro: target.repro,
            subject: target.subject,
        });
    }
    (witnesses, selected_contract_names, selected_file_paths)
}

fn select_group_contracts(
    root: &Path,
    report: &AuditModuleReport,
    selected_contract_names: &BTreeSet<&str>,
    filters: &[PathBuf],
    selected_file_paths: &mut BTreeSet<String>,
) -> Vec<AuditContractOutput> {
    let mut contracts = Vec::new();
    for contract in &report.contracts {
        if !selected_contract_names.contains(contract.name.as_str()) {
            continue;
        }
        for path in &contract.paths {
            let candidate = Path::new(path);
            if root.join(candidate).is_file()
                && is_matching_finding(&report.name, Some(candidate), filters)
            {
                let _inserted = selected_file_paths.insert(format_slash_path(candidate));
            }
        }
        contracts.push(contract.clone());
    }
    contracts
}

fn select_group_test_files(
    module_name: &str,
    files: &[String],
    discovered_modules: &[AuditModuleInput],
) -> Vec<String> {
    if let Some(module) = discovered_modules
        .iter()
        .find(|item| item.name == module_name)
        && !module.test_files.is_empty()
    {
        return module.test_files.clone();
    }
    let selected_files: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let mut seen_tests = BTreeSet::new();
    let mut tests = Vec::new();
    let overlapping = discovered_modules
        .iter()
        .filter(|module| {
            module
                .files
                .iter()
                .any(|path| selected_files.contains(path.as_str()))
        })
        .flat_map(|module| module.test_files.iter());
    for test_file in overlapping {
        if tests.len() >= MAX_GROUP_TEST_FILES {
            break;
        }
        if seen_tests.insert(test_file.as_str()) {
            tests.push(test_file.clone());
        }
    }
    tests
}
