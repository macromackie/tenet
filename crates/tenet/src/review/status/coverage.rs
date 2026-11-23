use alloc::collections::BTreeSet;
use std::{
    io::Write,
    path::{Component, Path, PathBuf},
};

use serde::Serialize;
use tenet_checks::behavior::{find_pin_path, read_pin_summary};
use tenet_contracts::Contract;
use tenet_engine::{discover_contracts, list_paths};

use crate::{error::CommandError, output, report_format::format_dim};

const MAX_DISPLAY_UNCOVERED_DIRS: usize = 6;

/// Offline governance summary for one discovered contract.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ContractCoverage {
    pub ephemeral: bool,
    pub governed_files: usize,
    pub name: String,
    pub path: PathBuf,
    pub scope: PathBuf,
    pub script_count: usize,
}

/// Offline repository coverage computed from `.contracts/` and optional `.tenet/scan/.contracts/`.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct RepoCoverage {
    pub contracts: Vec<ContractCoverage>,
    pub governed_files: usize,
    pub total_files: usize,
    pub uncovered_dirs: Vec<String>,
    pub uncovered_files: usize,
}

/// Discover committed `.contracts/` under `root` and optional ephemeral contracts under
/// `<state>/scan/.contracts/`.
pub(crate) fn discover_all_contracts(
    root: &Path,
    state: &Path,
) -> Result<(Vec<Contract>, Vec<Contract>), CommandError> {
    let committed = discover_contracts(root)?;
    let scan_root = state.join("scan");
    let ephemeral = if scan_root.join(".contracts").is_dir() {
        discover_contracts(&scan_root)?
    } else {
        Vec::new()
    };
    Ok((committed, ephemeral))
}

/// Compute offline coverage across committed `.contracts/` and ephemeral
/// `<state>/scan/.contracts/`.
pub(crate) fn compute_coverage(root: &Path, state: &Path) -> Result<RepoCoverage, CommandError> {
    let source_files = collect_source_files(root)?;
    let (committed, ephemeral) = discover_all_contracts(root, state)?;
    let scan_root = state.join("scan");
    let mut governed_paths = BTreeSet::new();
    let mut contracts = Vec::with_capacity(committed.len().saturating_add(ephemeral.len()));
    for contract in &committed {
        contracts.push(summarize_contract(
            root,
            contract,
            false,
            &source_files,
            &mut governed_paths,
        ));
    }
    for contract in &ephemeral {
        contracts.push(summarize_contract(
            &scan_root,
            contract,
            true,
            &source_files,
            &mut governed_paths,
        ));
    }
    let uncovered_dirs = collect_uncovered_dirs(&source_files, &governed_paths);
    let total_files = source_files.len();
    let governed_files = governed_paths.len();
    let uncovered_files = total_files.saturating_sub(governed_files);
    Ok(RepoCoverage {
        contracts,
        governed_files,
        total_files,
        uncovered_dirs,
        uncovered_files,
    })
}

/// Format a crisp one-line coverage summary for offline checks and status footers.
pub(crate) fn format_coverage_line(coverage: &RepoCoverage) -> String {
    format!(
        "{} governed \u{b7} {} uncovered source files",
        coverage.governed_files, coverage.uncovered_files,
    )
}

/// Render the `Coverage` block in terminal `tenet status` output.
pub(super) fn write_coverage_section(
    out: &mut impl Write,
    coverage: &RepoCoverage,
) -> Result<(), CommandError> {
    writeln!(out, "\n {}", output::format_styled("Coverage").bold())?;
    let total_scripts: usize = coverage
        .contracts
        .iter()
        .map(|contract| contract.script_count)
        .sum();
    let contract_noun = choose_plural(coverage.contracts.len(), "contract", "contracts");
    let script_noun = choose_plural(total_scripts, "script", "scripts");
    writeln!(
        out,
        "  {}/{} source files governed by {} {contract_noun} {}",
        coverage.governed_files,
        coverage.total_files,
        coverage.contracts.len(),
        format_dim(&format!("({total_scripts} pinned {script_noun})")),
    )?;
    for contract in &coverage.contracts {
        let ephemeral_tag = if contract.ephemeral {
            " (ephemeral)"
        } else {
            ""
        };
        let scope_label = if contract.scope.as_os_str().is_empty() {
            String::from(".")
        } else {
            contract.scope.display().to_string()
        };
        writeln!(
            out,
            "    {}  {}",
            contract.name,
            format_dim(&format!(
                "scope {scope_label} \u{b7} {} files \u{b7} {} scripts{ephemeral_tag}",
                contract.governed_files, contract.script_count,
            )),
        )?;
    }
    write_uncovered_dirs(out, &coverage.uncovered_dirs)
}

fn write_uncovered_dirs(
    out: &mut impl Write,
    uncovered_dirs: &[String],
) -> Result<(), CommandError> {
    if uncovered_dirs.is_empty() {
        return Ok(());
    }
    let shown: Vec<&str> = uncovered_dirs
        .iter()
        .take(MAX_DISPLAY_UNCOVERED_DIRS)
        .map(String::as_str)
        .collect();
    let extra = uncovered_dirs
        .len()
        .saturating_sub(MAX_DISPLAY_UNCOVERED_DIRS);
    let suffix = if extra > 0 {
        format!(" (+{extra} more)")
    } else {
        String::new()
    };
    writeln!(
        out,
        "  {}  {}{suffix}",
        format_dim("Uncovered"),
        shown.join(", "),
    )?;
    Ok(())
}

fn collect_source_files(root: &Path) -> Result<Vec<PathBuf>, CommandError> {
    let paths = list_paths(root)?;
    Ok(paths
        .into_iter()
        .filter(|path| !is_contract_path(path) && !is_dotfile(path))
        .collect())
}

fn collect_uncovered_dirs(
    source_files: &[PathBuf],
    governed_paths: &BTreeSet<PathBuf>,
) -> Vec<String> {
    let mut dirs = BTreeSet::new();
    for file in source_files {
        if governed_paths.contains(file) {
            continue;
        }
        let dir = match file.parent().and_then(|parent| parent.components().next()) {
            Some(Component::Normal(segment)) => segment.to_string_lossy().into_owned(),
            _ => String::from("."),
        };
        let _inserted = dirs.insert(dir);
    }
    dirs.into_iter().collect()
}

fn is_contract_path(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == ".contracts")
}

fn is_dotfile(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
}

const fn choose_plural(count: usize, singular: &'static str, plural: &'static str) -> &'static str {
    if count == 1 { singular } else { plural }
}

fn summarize_contract(
    base_dir: &Path,
    contract: &Contract,
    ephemeral: bool,
    source_files: &[PathBuf],
    governed_paths: &mut BTreeSet<PathBuf>,
) -> ContractCoverage {
    let contract_dir = contract
        .path
        .parent()
        .map_or_else(|| base_dir.to_path_buf(), |parent| base_dir.join(parent));
    let script_count = find_pin_path(&contract_dir)
        .and_then(|pin| read_pin_summary(&pin).ok())
        .map_or(0, |summary| summary.check_count);
    let mut governed_files = 0_usize;
    for file in source_files {
        if contract.is_governing(file) {
            governed_files = governed_files.saturating_add(1);
            let _inserted = governed_paths.insert(file.clone());
        }
    }
    ContractCoverage {
        ephemeral,
        governed_files,
        name: contract.name.clone(),
        path: contract.path.clone(),
        scope: contract.scope.clone(),
        script_count,
    }
}
