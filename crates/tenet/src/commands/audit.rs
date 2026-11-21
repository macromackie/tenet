mod discover;
mod queue;
mod render;
mod save;
#[cfg(test)]
mod tests;

use std::{env, io::IsTerminal as _, path::Path, time::Instant};

use anyhow::{Context as _, anyhow};
use console::Term;
use discover::discover_modules;
use queue::AuditQueue;
use render::write_report;
use save::save_verified_contracts;
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

/// Discover modules, synthesize contracts, falsify them in isolated sandboxes, and report defects.
pub(super) fn run(root: &Path, state: &Path, args: &Audit) -> Result<ExitStatus, CommandError> {
    require_credentials()?;
    let budget = usize::try_from(args.max_requests).context("convert audit request budget")?;
    let discovered = discover_modules(root, &args.paths)?;
    if discovered.modules.is_empty() {
        return Err(anyhow!("no supported production source files were found to audit").into());
    }
    let mut queue = AuditQueue::read(state, args.deep, args.fresh)?;
    let pending: Vec<_> = discovered
        .modules
        .iter()
        .filter(|module| !queue.has_module(&module.name))
        .cloned()
        .collect();
    let transient = if !pending.is_empty() && budget > 0 {
        run_pending_modules(
            root,
            state,
            args,
            budget,
            pending,
            &discovered.runtime_paths,
            &mut queue,
        )?
    } else {
        Vec::new()
    };
    let selected_reports = select_reports(&discovered.modules, &queue, &transient);
    let saved = if args.save {
        save_verified_contracts(root, state, &selected_reports)?
    } else {
        Vec::new()
    };
    let mut out = output::open_stdout().lock();
    write_report(&mut out, args.deep, &selected_reports, &saved, args.json)?;
    Ok(read_exit_status(&selected_reports))
}

fn is_transient_synthesis_failure(report: &AuditModuleReport) -> bool {
    report.contracts.is_empty()
        && report
            .findings
            .iter()
            .any(|row| row.check == "synthesis" && row.outcome == Outcome::Unresolved)
}

fn read_exit_status(reports: &[AuditModuleReport]) -> ExitStatus {
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

fn require_credentials() -> Result<(), CommandError> {
    let has_key = env::var_os("OPENROUTER_API_KEY").is_some_and(|value| !value.is_empty());
    if has_key {
        return Ok(());
    }
    Err(anyhow!("set \"OPENROUTER_API_KEY\" to run `tenet audit`").into())
}

fn run_pending_modules(
    root: &Path,
    state: &Path,
    args: &Audit,
    budget: usize,
    pending: Vec<AuditModuleInput>,
    runtime_paths: &[String],
    queue: &mut AuditQueue,
) -> Result<Vec<AuditModuleReport>, CommandError> {
    let mut remaining = budget;
    let started = Instant::now();
    let term = Term::stderr();
    let show_progress = !args.json && output::open_stderr().is_terminal();
    let mut progress = Progress::default();
    let mut transient = Vec::new();
    for (index, module) in pending.into_iter().enumerate() {
        if remaining == 0 {
            break;
        }
        let summary = format!("Audited {index} · budget left {remaining}");
        let mut on_progress = |label: &str| {
            drop(progress.draw(&term, label, &summary, started.elapsed()));
        };
        let callback: Option<&mut dyn FnMut(&str)> = show_progress.then_some(&mut on_progress);
        let request = AuditRequest {
            deep: args.deep,
            fresh: args.fresh,
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
        queue.save(state)?;
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
