mod merge;
#[cfg(test)]
mod merge_tests;
mod scripts;
#[cfg(test)]
mod tests;

use alloc::collections::BTreeSet;
use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

use scripts::{
    FilenamePolicy,
    SUPPORTED_PIN_PROTOCOL,
    read_mutable_checks,
    read_pin_json,
    write_check_entry,
    write_pretty_pin_lock,
};
use serde::Deserialize;

/// Relative path under a contract folder to the legacy inline behavior-check lockfile.
pub const LEGACY_LOCK_FILE: &str = "agent-checks.lock.json";
/// Directory name under a contract folder holding runnable behavior-check scripts and lockfile.
pub const SCRIPTS_DIR: &str = "scripts";
/// Relative path under a contract folder to the unified behavior-check lockfile.
pub const SCRIPTS_LOCK_FILE: &str = "scripts/.lock.json";

/// Outcome of staging a behavior-check pin bundle into a contract folder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PinBundleState {
    Missing,
    Written,
}

/// Summary of a parsed contract behavior-check pin lockfile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PinSummary {
    pub check_count: usize,
    pub checks: Vec<PinnedCheck>,
    pub lock_path: PathBuf,
    pub protocol: u64,
}

/// Metadata for one pinned behavior check declared in a contract lockfile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PinnedCheck {
    pub id: String,
    pub promise: String,
    pub runtime: String,
    pub script_file: Option<String>,
    pub script_hash: Option<String>,
    pub subject: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawBundle {
    checks: Vec<RawPinnedCheck>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPin {
    bundle: RawBundle,
    protocol: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPinnedCheck {
    id: String,
    promise: String,
    runtime: String,
    #[serde(default)]
    script_file: Option<String>,
    #[serde(default)]
    script_hash: Option<String>,
    subject: String,
}

/// Copy a staged behavior-check pin bundle into `destination_contract_dir/scripts/`.
///
/// Copies referenced sibling scripts or materializes legacy inline checks into standalone
/// script files while leaving any existing destination scripts untouched and stripping
/// bulky control source copies into `controlHash`.
#[inline]
pub fn copy_staged_pin_bundle(
    source_lock: &Path,
    destination_contract_dir: &Path,
) -> io::Result<PinBundleState> {
    let Some(mut pin_value) = read_pin_json(source_lock)? else {
        return Ok(PinBundleState::Missing);
    };
    let source_dir = source_lock.parent().unwrap_or_else(|| Path::new("."));
    let destination_scripts_dir = destination_contract_dir.join(SCRIPTS_DIR);
    fs::create_dir_all(&destination_scripts_dir)?;
    let checks = read_mutable_checks(&mut pin_value, source_lock)?;
    let mut used_filenames = BTreeSet::new();
    for check in checks {
        write_check_entry(
            check,
            source_dir,
            &destination_scripts_dir,
            &mut used_filenames,
            FilenamePolicy::Keep,
        )?;
    }
    write_pretty_pin_lock(destination_contract_dir, &pin_value)?;
    Ok(PinBundleState::Written)
}

/// Locate the active behavior-check lockfile inside `contract_dir`, preferring
/// `scripts/.lock.json` over the legacy `agent-checks.lock.json`.
#[inline]
#[must_use]
pub fn find_pin_path(contract_dir: &Path) -> Option<PathBuf> {
    let scripts_lock = contract_dir.join(SCRIPTS_LOCK_FILE);
    if scripts_lock.is_file() {
        return Some(scripts_lock);
    }
    let legacy_lock = contract_dir.join(LEGACY_LOCK_FILE);
    if legacy_lock.is_file() {
        return Some(legacy_lock);
    }
    None
}

/// Merge checks from multiple staged pin lockfiles into one unified `scripts/` folder and
/// slim `scripts/.lock.json` inside `destination_contract_dir`.
///
/// Deduplicates colliding check identifiers and script filenames, preserves hand-edited
/// destination scripts, strips bulky control source copies into `controlHash`, records
/// `contractHash`, and combines source `shape` values deterministically.
#[inline]
pub fn merge_staged_pin_bundles(
    source_locks: &[PathBuf],
    destination_contract_dir: &Path,
    contract_hash: &str,
) -> io::Result<PinBundleState> {
    merge::merge_staged_pin_bundles(source_locks, destination_contract_dir, contract_hash)
}

/// Read and validate a contract behavior-check pin lockfile into a structured summary.
#[inline]
pub fn read_pin_summary(lock_path: &Path) -> Result<PinSummary, String> {
    let raw_text = fs::read_to_string(lock_path).map_err(|error| {
        format!(
            "could not read behavior pin {:?}: {error:#}",
            lock_path.display()
        )
    })?;
    let parsed: RawPin = serde_json::from_str(&raw_text).map_err(|error| {
        format!(
            "invalid behavior pin JSON {:?}: {error:#}",
            lock_path.display()
        )
    })?;
    if parsed.protocol != SUPPORTED_PIN_PROTOCOL {
        return Err(format!(
            "unsupported behavior pin protocol {:?} in {:?}",
            parsed.protocol,
            lock_path.display()
        ));
    }
    let checks: Vec<PinnedCheck> = parsed
        .bundle
        .checks
        .into_iter()
        .map(|entry| {
            PinnedCheck {
                id: entry.id,
                promise: entry.promise,
                runtime: entry.runtime,
                script_file: entry.script_file,
                script_hash: entry.script_hash,
                subject: entry.subject,
            }
        })
        .collect();
    Ok(PinSummary {
        check_count: checks.len(),
        checks,
        lock_path: lock_path.to_path_buf(),
        protocol: parsed.protocol,
    })
}
