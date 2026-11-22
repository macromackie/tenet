use alloc::collections::BTreeMap;
use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use tenet_checks::behavior::{AuditModuleReport, Outcome};

use crate::error::CommandError;

const MAX_SLUG_LEN: usize = 48;

/// Materialize runnable witness reproducer scripts under `dir` and return their written paths.
pub(super) fn write_witness_repros(
    dir: &Path,
    reports: &[AuditModuleReport],
) -> Result<BTreeMap<(String, String), PathBuf>, CommandError> {
    let mut written_paths = BTreeMap::new();
    let mut index = 0_usize;
    for report in reports {
        for finding in &report.findings {
            let Some(repro) = finding.repro.as_ref() else {
                continue;
            };
            if finding.outcome != Outcome::Failed {
                continue;
            }
            if index == 0 {
                fs::create_dir_all(dir)
                    .with_context(|| format!("create repro directory {:?}", dir.display()))?;
            }
            index = index.saturating_add(1);
            let filename =
                build_repro_filename(index, &finding.contract, &finding.check, &repro.runtime);
            let path = dir.join(filename);
            fs::write(&path, &repro.code)
                .with_context(|| format!("write witness repro {:?}", path.display()))?;
            let _prior =
                written_paths.insert((finding.contract.clone(), finding.check.clone()), path);
        }
    }
    Ok(written_paths)
}

fn build_repro_filename(index: usize, contract: &str, check: &str, runtime: &str) -> String {
    let safe_contract = format_slug(contract);
    let safe_check = format_slug(check);
    let extension = read_runtime_extension(runtime);
    format!("{index:02}-{safe_contract}-{safe_check}.{extension}")
}

fn format_slug(raw: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in raw.chars() {
        if slug.len() >= MAX_SLUG_LEN {
            break;
        }
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
            continue;
        }
        if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    let trimmed = slug.trim_end_matches('-');
    if trimmed.is_empty() {
        return String::from("witness");
    }
    trimmed.to_owned()
}

fn read_runtime_extension(runtime: &str) -> &'static str {
    match runtime {
        "go" => "go",
        "node" => "mjs",
        "python" => "py",
        "rust" => "rs",
        _ => "txt",
    }
}
