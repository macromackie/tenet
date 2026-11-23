mod discover;
mod queue;
mod render;
mod replay;
mod repros;
mod save;
#[cfg(test)]
mod tests;

use alloc::collections::BTreeMap;
use std::{
    env,
    io::IsTerminal as _,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context as _, anyhow};
use console::Term;
pub(super) use discover::{
    discover_modules,
    find_related_files,
    format_slash_path,
    is_matching_filter,
    normalize_filters,
};
pub(super) use queue::AuditQueue;
use render::write_report;
pub(super) use render::write_witness_header;
pub(super) use replay::{
    StatusRow,
    build_contract_lock_paths,
    build_replay_target,
    is_matching_finding,
    write_replay_results,
    write_status_row,
};
pub(super) use repros::write_witness_repros;
use save::save_verified_contracts;
pub(super) use save::{
    SavedContract,
    read_global_contract_names,
    save_contracts_to,
    strip_saved_path_prefix,
};
use tenet_checks::behavior::{
    AuditModuleInput,
    AuditModuleReport,
    AuditRequest,
    Outcome,
    run_audit,
};

use crate::{
    cli::{Audit, ExitStatus},
    error::CommandError,
    output,
    progress::Progress,
};

const MIN_FOLLOWUP_MODULE_BUDGET: usize = 2;

/// Parameters for running module discovery and synthesis/falsification shared by `audit` and
/// `scan`.
pub(super) struct AuditRunParams<'params> {
    pub base: Option<&'params str>,
    pub deep: bool,
    pub fresh: bool,
    pub json: bool,
    pub max_requests: usize,
    pub paths: &'params [PathBuf],
}

/// Return whether `OPENROUTER_API_KEY` is populated in the current environment.
pub(super) fn has_openrouter_key() -> bool {
    env::var_os("OPENROUTER_API_KEY").is_some_and(|value| !value.is_empty())
}

/// Verify that `OPENROUTER_API_KEY` is populated before running a model-backed `command`.
pub(super) fn require_openrouter_key(command: &str) -> Result<(), CommandError> {
    if has_openrouter_key() {
        return Ok(());
    }
    Err(anyhow!("set \"OPENROUTER_API_KEY\" to run {command:?}").into())
}

/// Discover production modules and execute bounded synthesis/falsification, persisting the
/// resumable queue in `state` when running outside `--base` mode.
pub(super) fn execute_module_audit(
    root: &Path,
    state: &Path,
    params: &AuditRunParams<'_>,
) -> Result<(Vec<AuditModuleReport>, bool), CommandError> {
    let discovered = discover_modules(root, params.paths, params.base)?;
    if discovered.modules.is_empty() {
        return Ok((Vec::new(), false));
    }
    let persist_queue = params.base.is_none();
    let mut queue = if persist_queue {
        AuditQueue::read(state, params.deep, params.fresh)?
    } else {
        AuditQueue::new(params.deep)
    };
    let pending: Vec<_> = discovered
        .modules
        .iter()
        .filter(|module| !queue.has_module(&module.name))
        .cloned()
        .collect();
    let transient = if !pending.is_empty() && params.max_requests > 0 {
        run_pending_modules(
            root,
            state,
            params,
            pending,
            &discovered.runtime_paths,
            &mut queue,
        )?
    } else {
        Vec::new()
    };
    Ok((
        select_reports(&discovered.modules, &queue, &transient),
        true,
    ))
}

/// Discover modules, synthesize contracts, falsify them in isolated sandboxes, and report defects.
pub(super) fn run(root: &Path, state: &Path, args: &Audit) -> Result<ExitStatus, CommandError> {
    if args.replay {
        return replay::run(root, state, args);
    }
    require_openrouter_key("tenet audit")?;
    let budget = usize::try_from(args.max_requests).context("convert audit request budget")?;
    let params = AuditRunParams {
        base: args.base.as_deref(),
        deep: args.deep,
        fresh: args.fresh,
        json: args.json,
        max_requests: budget,
        paths: &args.paths,
    };
    let (selected_reports, found_modules) = execute_module_audit(root, state, &params)?;
    if !found_modules {
        if args.base.is_some() {
            let mut out = output::open_stdout().lock();
            write_report(&mut out, args.deep, &[], &[], &BTreeMap::new(), args.json)?;
            return Ok(ExitStatus::Passed);
        }
        return Err(anyhow!("no supported production source files were found to audit").into());
    }
    finish_audit(root, state, args, &selected_reports)
}

/// Execute one synchronous worker call while drawing live progress on stderr when interactive.
pub(super) fn run_with_progress<ResultValue>(
    json_mode: bool,
    summary: &str,
    work: impl FnOnce(Option<&mut dyn FnMut(&str)>) -> Result<ResultValue, CommandError>,
) -> Result<ResultValue, CommandError> {
    let started = Instant::now();
    let term = Term::stderr();
    let show_progress = !json_mode && output::open_stderr().is_terminal();
    let mut progress = Progress::default();
    let mut on_progress = |label: &str| {
        drop(progress.draw(&term, label, summary, started.elapsed()));
    };
    let callback: Option<&mut dyn FnMut(&str)> = show_progress.then_some(&mut on_progress);
    let outcome = work(callback);
    if show_progress {
        drop(progress.clear(&term));
    }
    outcome
}

fn finish_audit(
    root: &Path,
    state: &Path,
    args: &Audit,
    selected_reports: &[AuditModuleReport],
) -> Result<ExitStatus, CommandError> {
    let saved = if args.save {
        save_verified_contracts(root, state, selected_reports)?
    } else {
        Vec::new()
    };
    let repro_paths = match args.repros_dir.as_deref() {
        Some(dir) => write_witness_repros(dir, selected_reports)?,
        None => BTreeMap::new(),
    };
    let mut out = output::open_stdout().lock();
    write_report(
        &mut out,
        args.deep,
        selected_reports,
        &saved,
        &repro_paths,
        args.json,
    )?;
    Ok(read_exit_status(selected_reports))
}

fn is_transient_synthesis_failure(report: &AuditModuleReport) -> bool {
    report.contracts.is_empty()
        && report
            .findings
            .iter()
            .any(|row| row.check == "synthesis" && row.outcome == Outcome::Unresolved)
}

pub(super) fn read_exit_status(reports: &[AuditModuleReport]) -> ExitStatus {
    let has_failures = reports.iter().any(|report| {
        report
            .findings
            .iter()
            .any(|row| row.outcome == Outcome::Failed)
    });
    if has_failures {
        return ExitStatus::Failed;
    }
    if reports.iter().any(is_transient_synthesis_failure) {
        return ExitStatus::Pending;
    }
    ExitStatus::Passed
}

fn run_pending_modules(
    root: &Path,
    state: &Path,
    params: &AuditRunParams<'_>,
    pending: Vec<AuditModuleInput>,
    runtime_paths: &[String],
    queue: &mut AuditQueue,
) -> Result<Vec<AuditModuleReport>, CommandError> {
    let persist_queue = params.base.is_none();
    let mut remaining = params.max_requests;
    let started = Instant::now();
    let term = Term::stderr();
    let show_progress = !params.json && output::open_stderr().is_terminal();
    let mut progress = Progress::default();
    let mut transient = Vec::new();
    for (index, module) in pending.into_iter().enumerate() {
        if remaining == 0 || (index > 0 && remaining < MIN_FOLLOWUP_MODULE_BUDGET) {
            break;
        }
        let summary = format!("Audited {index} · budget left {remaining}");
        let mut on_progress = |label: &str| {
            drop(progress.draw(&term, label, &summary, started.elapsed()));
        };
        let callback: Option<&mut dyn FnMut(&str)> = show_progress.then_some(&mut on_progress);
        let request = AuditRequest {
            deep: params.deep,
            fresh: params.fresh,
            max_requests: remaining,
            modules: vec![module],
            root: root.to_path_buf(),
            runtime_paths: runtime_paths.to_vec(),
            state: state.to_path_buf(),
        };
        let response = run_audit(&request, callback).context("run isolated audit worker")?;
        remaining = remaining.saturating_sub(response.requests);
        for report in response.modules {
            if is_transient_synthesis_failure(&report) {
                transient.push(report);
            } else {
                queue.upsert(report);
            }
        }
        if persist_queue {
            queue.save(state)?;
        }
    }
    if show_progress {
        drop(progress.clear(&term));
    }
    Ok(transient)
}

fn select_reports(
    modules: &[AuditModuleInput],
    queue: &AuditQueue,
    transient: &[AuditModuleReport],
) -> Vec<AuditModuleReport> {
    modules
        .iter()
        .filter_map(|module| {
            queue
                .modules
                .get(&module.name)
                .or_else(|| transient.iter().find(|report| report.name == module.name))
                .cloned()
        })
        .collect()
}
