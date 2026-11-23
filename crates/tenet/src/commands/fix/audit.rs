mod groups;
mod render;
#[cfg(test)]
mod tests;

use std::path::Path;

use anyhow::Context as _;
use groups::build_fix_groups;
use render::write_fix_report;
use tenet_checks::{
    behavior::{
        AuditFixGroup,
        AuditFixOutcome,
        AuditFixRequest,
        AuditFixResponse,
        AuditFixStatus,
        run_audit_fix,
    },
    fixes,
};

use super::super::audit::{
    AuditQueue,
    discover_modules,
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
