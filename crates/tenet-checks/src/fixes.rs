//! Safe native fixes for adopted contract assets and the selected project sources.

mod native;
mod observation;
mod staging;

use alloc::collections::BTreeSet;
use std::{io, path::PathBuf};

use native::{Job, Tool};
use serde::Serialize;
use staging::Staging;
use tenet_contracts::hash_contract;
use tenet_engine::{LockFileError, Plan, read_lock};

/// A native tool's verification on the final staged files.
#[derive(Debug, Serialize)]
pub struct Verification {
    /// The contract that selected this configuration.
    pub contract: String,
    /// First remaining finding, with its location where supplied.
    pub finding: Option<String>,
    /// Whether the native linter found no remaining violations.
    pub passed: bool,
    /// The linter used.
    pub tool: &'static str,
}

/// One native checker that could not inspect a contract's selected inputs.
#[derive(Debug, Serialize)]
pub struct Unresolved {
    /// The contract whose check could not run.
    pub contract: String,
    /// The setup or execution problem.
    pub message: String,
    /// The selected native tool.
    pub tool: &'static str,
}

/// Files changed by safe fixes and their independent read-only reruns.
#[derive(Debug, Default, Serialize)]
pub struct Report {
    /// Unified diffs, relative to the project root.
    pub changes: Vec<Change>,
    /// Contracts with applicable source and no supported native fixer.
    pub manual: Vec<String>,
    /// Selected native checks that could not provide an observation.
    pub unresolved: Vec<Unresolved>,
    /// Read-only results after all fixes finished.
    pub verified: Vec<Verification>,
}

/// One applied change.
#[derive(Debug, Serialize)]
pub struct Change {
    /// The applied unified diff.
    pub patch: String,
    /// The project-relative file path.
    pub path: PathBuf,
}

/// A fix could not safely complete.
#[derive(Debug, thiserror::Error)]
pub enum FixError {
    /// An installed tool, selection, source or configuration could not be used.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// The adoption lock cannot identify the installed assets.
    #[error(transparent)]
    Lock(#[from] LockFileError),
}

/// Apply safe built-in fixes on disposable copies, rerun every selected linter on the final
/// copies, and replace only changed source files whose original bytes still match.
#[inline]
pub async fn apply(plan: &Plan) -> Result<Report, FixError> {
    let (staging, jobs, mut report) = prepare(plan)?;
    if !report.unresolved.is_empty() {
        return Ok(report);
    }
    for job in &jobs {
        if let Err(error) = job.observe(staging.tree(), true).await {
            report.unresolved.push(job.report_unresolved(&error));
            return Ok(report);
        }
    }
    verify(&staging, &jobs, &mut report).await;
    if report.unresolved.is_empty() {
        report.changes = staging.apply()?;
    }
    Ok(report)
}

/// Run adopted native lint rules on copies of the selected sources without modifying the
/// checkout. A finding on a changed file can have existed before the change.
#[inline]
pub async fn inspect(plan: &Plan) -> Result<Report, FixError> {
    let (staging, jobs, mut report) = prepare(plan)?;
    verify(&staging, &jobs, &mut report).await;
    staging.check_sources()?;
    Ok(report)
}

async fn verify(staging: &Staging<'_>, jobs: &[Job], report: &mut Report) {
    for job in jobs {
        match job.observe(staging.tree(), false).await {
            Ok(observation) => report.verified.push(observation),
            Err(error) => report.unresolved.push(job.report_unresolved(&error)),
        }
    }
}

fn prepare(plan: &Plan) -> Result<(Staging<'_>, Vec<Job>, Report), FixError> {
    let installed = read_lock(&plan.root)?;
    let mut staging = Staging::new(&plan.root)?;
    let mut jobs = Vec::new();
    let mut report = Report::default();
    let deleted_paths: BTreeSet<_> = plan.deleted.iter().collect();

    for contract in &plan.contracts {
        let sources: Vec<_> = plan
            .files
            .iter()
            .filter(|path| {
                contract.is_governing(path)
                    && !deleted_paths.contains(path)
                    && !path.iter().any(|part| part == ".contracts")
            })
            .collect();
        if sources.is_empty() {
            continue;
        }
        prepare_contract(
            contract,
            installed.contracts.get(&contract.path),
            &sources,
            &mut staging,
            &mut jobs,
            &mut report,
        );
    }
    Ok((staging, jobs, report))
}

fn prepare_contract(
    contract: &tenet_contracts::Contract,
    installation: Option<&tenet_contracts::Entry>,
    sources: &[&PathBuf],
    staging: &mut Staging<'_>,
    jobs: &mut Vec<Job>,
    report: &mut Report,
) {
    // Local adaptations may narrow a rule in prose beyond what its installed linter expresses.
    let Some(entry) = installation
        .filter(|entry| hash_contract(&contract.document).is_ok_and(|hash| hash == entry.hash))
    else {
        report.manual.push(contract.name.clone());
        return;
    };
    let mut selected = false;
    for tool in [Tool::Oxlint, Tool::Ruff] {
        let Some(hash) = entry.files.get(tool.find_asset_path()) else {
            continue;
        };
        let targets = sources
            .iter()
            .copied()
            .filter(|path| tool.can_check(path))
            .cloned()
            .collect::<Vec<_>>();
        if targets.is_empty() {
            continue;
        }
        match staging.prepare_job(contract, tool, hash, jobs.len(), targets) {
            Ok(Some(job)) => {
                jobs.push(job);
                selected = true;
            },
            Ok(None) => {},
            Err(error) => {
                selected = true;
                report.unresolved.push(Unresolved {
                    contract: contract.name.clone(),
                    message: error.to_string(),
                    tool: tool.name(),
                });
            },
        }
    }
    if !selected {
        report.manual.push(contract.name.clone());
    }
}
