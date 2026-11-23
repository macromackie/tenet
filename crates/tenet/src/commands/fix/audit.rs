mod render;

use alloc::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use render::write_fix_report;
use tenet_checks::{
    behavior::{
        AuditContractOutput,
        AuditFixGroup,
        AuditFixOutcome,
        AuditFixRequest,
        AuditFixResponse,
        AuditFixStatus,
        AuditFixWitness,
        AuditModuleInput,
        AuditModuleReport,
        AuditReplayTarget,
        Outcome,
        run_audit_fix,
    },
    fixes,
};

use super::super::audit::{
    AuditQueue,
    build_contract_lock_paths,
    build_replay_target,
    discover_modules,
    find_related_files,
    format_slash_path,
    is_matching_finding,
    normalize_filters,
    require_openrouter_key,
    run_with_progress,
};
use crate::{
    cli::{ExitStatus, Fix},
    error::CommandError,
    output,
};

/// Repair failing witnesses from `.tenet/audit.json` in a bounded sandboxed verify-until-green
/// loop and apply verified patches atomically to the checkout.
pub(super) fn run(root: &Path, state: &Path, args: &Fix) -> Result<ExitStatus, CommandError> {
    require_openrouter_key("tenet fix --from-audit")?;
    let max_requests = usize::try_from(args.max_requests).context("convert fix request budget")?;
    let queue = AuditQueue::read_existing(state)?;
    let filters = normalize_filters(&args.paths)?;
    let discovered = discover_modules(root, &[], None)?;
    let groups = build_fix_groups(
        root,
        &queue,
        args.contract.as_deref(),
        &filters,
        &discovered.modules,
        &discovered.runtime_paths,
    );
    let (response, changes) = if groups.is_empty() {
        (
            AuditFixResponse {
                cost_usd: None,
                outcomes: Vec::new(),
                patches: Vec::new(),
                requests: 0,
            },
            Vec::new(),
        )
    } else {
        execute_audit_fix(
            root,
            state,
            args.json,
            queue.deep,
            max_requests,
            groups,
            discovered.runtime_paths,
        )?
    };
    let status = read_fix_exit_status(&response.outcomes);
    let mut out = output::open_stdout().lock();
    write_fix_report(&mut out, &response, &changes, status, args.json)?;
    Ok(status)
}

fn build_fix_groups(
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

fn build_module_group(
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
    Some(AuditFixGroup {
        contracts,
        files,
        guards,
        name: report.name.clone(),
        related_files,
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

fn execute_audit_fix(
    root: &Path,
    state: &Path,
    json_mode: bool,
    deep: bool,
    max_requests: usize,
    groups: Vec<AuditFixGroup>,
    runtime_paths: Vec<String>,
) -> Result<(AuditFixResponse, Vec<fixes::Change>), CommandError> {
    let summary = format!("Repairing {} modules · budget {max_requests}", groups.len());
    let request = AuditFixRequest {
        deep,
        groups,
        max_requests,
        root: root.to_path_buf(),
        runtime_paths,
        state: state.to_path_buf(),
    };
    let response = run_with_progress(json_mode, &summary, |callback| {
        run_audit_fix(&request, callback)
            .context("run isolated audit fix worker")
            .map_err(CommandError::from)
    })?;
    let changes =
        fixes::apply_audit_patches(root, &response.patches).map_err(anyhow::Error::from)?;
    Ok((response, changes))
}

fn read_fix_exit_status(outcomes: &[AuditFixOutcome]) -> ExitStatus {
    if outcomes
        .iter()
        .any(|row| row.status == AuditFixStatus::Failed)
    {
        return ExitStatus::Failed;
    }
    if outcomes
        .iter()
        .any(|row| row.status == AuditFixStatus::Unresolved)
    {
        return ExitStatus::Unresolved;
    }
    ExitStatus::Passed
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
