mod render;

use alloc::collections::BTreeMap;
use std::path::{Path, PathBuf};

use render::{ScanOutput, write_scan_output};
use tenet_adoption::{self as adoption, Added, Catalog};
use tenet_checks::behavior::AuditModuleReport;
use tenet_engine::discover_contracts;

use super::{audit, init};
use crate::{
    cli::{ExitStatus, Scan},
    error::CommandError,
    output,
};

const DEFAULT_STARTER_NAME: &str = "project-invariants";

/// Discover catalog recommendations, synthesize module contracts, and optionally materialize them.
pub(super) fn run(root: &Path, state: &Path, args: &Scan) -> Result<ExitStatus, CommandError> {
    if let Some(starter_name) = &args.starter {
        let name = starter_name.as_deref().unwrap_or(DEFAULT_STARTER_NAME);
        return init::run_starter_command(root, &args.scope, name, args.json);
    }
    let normalized_scope = init::normalize_scope(root, &args.scope)?;
    let (catalog, catalog_note) = read_catalog(root, args.library.as_deref())?;
    let (reports, behavior_note) = run_behavior_scan(root, state, args)?;
    let repro_paths = match args.repros_dir.as_deref() {
        Some(dir) if !reports.is_empty() => audit::write_witness_repros(dir, &reports)?,
        _ => BTreeMap::new(),
    };
    let (installed, saved) = save_scan_outputs(
        root,
        state,
        &normalized_scope,
        args,
        catalog.as_ref(),
        &reports,
    )?;
    let mut notes = Vec::new();
    if let Some(note) = catalog_note {
        notes.push(note);
    }
    if let Some(note) = behavior_note {
        notes.push(note);
    }
    let status = audit::read_exit_status(&reports);
    let mut out = output::open_stdout().lock();
    let payload = ScanOutput {
        apply: args.apply,
        catalog: catalog.as_ref(),
        ephemeral: args.ephemeral,
        installed: &installed,
        notes: &notes,
        reports: &reports,
        repro_paths: &repro_paths,
        saved: &saved,
        scope: &args.scope,
        status,
    };
    write_scan_output(&mut out, &payload, args.json)?;
    Ok(status)
}

fn read_catalog(
    root: &Path,
    library: Option<&Path>,
) -> Result<(Option<Catalog>, Option<String>), CommandError> {
    match adoption::find_candidates(root, library) {
        Ok(mut catalog) => {
            catalog.candidates.truncate(init::MAX_INIT_CANDIDATES);
            Ok((Some(catalog), None))
        },
        Err(error) if library.is_none() => {
            Ok((
                None,
                Some(format!("Catalog lookup skipped offline: {error}")),
            ))
        },
        Err(error) => Err(error.into()),
    }
}

fn save_scan_outputs(
    root: &Path,
    state: &Path,
    normalized_scope: &Path,
    args: &Scan,
    catalog: Option<&Catalog>,
    reports: &[AuditModuleReport],
) -> Result<(Vec<Added>, Vec<audit::SavedContract>), CommandError> {
    if args.apply {
        let installed = match catalog {
            Some(found) if !found.candidates.is_empty() => {
                init::install_catalog_candidates(root, &args.scope, args.library.as_deref(), found)?
            },
            _ => Vec::new(),
        };
        let existing = audit::read_global_contract_names(root)?;
        let contracts_dir = root.join(normalized_scope).join(".contracts");
        let mut saved = audit::save_contracts_to(&contracts_dir, &existing, state, reports)?;
        audit::strip_saved_path_prefix(&mut saved, root);
        let _validated = discover_contracts(root)?;
        return Ok((installed, saved));
    }
    if args.ephemeral {
        let existing = audit::read_global_contract_names(root)?;
        let ephemeral_dir: PathBuf = state.join("scan").join(".contracts");
        let mut saved = audit::save_contracts_to(&ephemeral_dir, &existing, state, reports)?;
        audit::strip_saved_path_prefix(&mut saved, root);
        return Ok((Vec::new(), saved));
    }
    Ok((Vec::new(), Vec::new()))
}

fn run_behavior_scan(
    root: &Path,
    state: &Path,
    args: &Scan,
) -> Result<(Vec<AuditModuleReport>, Option<String>), CommandError> {
    let forced = args.ephemeral || args.deep || args.fresh;
    if !audit::has_openrouter_key() && !forced {
        return Ok((
            Vec::new(),
            Some(String::from(
                "Set OPENROUTER_API_KEY to synthesize module contracts and run behavioral checks.",
            )),
        ));
    }
    audit::require_openrouter_key("tenet scan")?;
    let params = audit::AuditRunParams {
        base: args.base.as_deref(),
        deep: args.deep,
        fresh: args.fresh,
        json: args.json,
        max_requests: args.max_requests,
        paths: &args.paths,
    };
    let (reports, _) = audit::execute_module_audit(root, state, &params)?;
    Ok((reports, None))
}
