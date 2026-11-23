#[cfg(test)]
mod tests;

use alloc::collections::BTreeSet;
use std::{
    fs,
    io,
    path::{Component, Path, PathBuf},
};

use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

/// Relative path under a contract folder to the legacy inline behavior-check lockfile.
pub const LEGACY_LOCK_FILE: &str = "agent-checks.lock.json";
/// Directory name under a contract folder holding runnable behavior-check scripts and lockfile.
pub const SCRIPTS_DIR: &str = "scripts";
/// Relative path under a contract folder to the unified behavior-check lockfile.
pub const SCRIPTS_LOCK_FILE: &str = "scripts/.lock.json";

const ALLOWED_SCRIPT_PUNCTUATION: &[char] = &['.', '_', '-'];
const MAX_SCRIPT_NAME_CHARS: usize = 128_usize;
const MAX_SLUG_CHARS: usize = 64_usize;
const SUPPORTED_PIN_PROTOCOL: u64 = 1_u64;

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
/// script files while leaving any existing destination scripts untouched.
#[inline]
pub fn copy_staged_pin_bundle(
    source_lock: &Path,
    destination_contract_dir: &Path,
) -> io::Result<PinBundleState> {
    let raw_text = match fs::read_to_string(source_lock) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(PinBundleState::Missing);
        },
        Err(error) => return Err(error),
    };
    let mut pin_value: Value = serde_json::from_str(&raw_text).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "invalid behavior pin JSON {:?}: {error:#}",
                source_lock.display()
            ),
        )
    })?;
    validate_pin_protocol(&pin_value, source_lock)?;
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
        )?;
    }
    let mut encoded = serde_json::to_string_pretty(&pin_value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    encoded.push('\n');
    fs::write(destination_contract_dir.join(SCRIPTS_LOCK_FILE), encoded)?;
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

fn allocate_script_filename(
    check_id: &str,
    runtime: &str,
    used_filenames: &mut BTreeSet<String>,
) -> String {
    let slug = format_check_slug(check_id);
    let extension = resolve_runtime_extension(runtime);
    let candidate = format!("{slug}.{extension}");
    if used_filenames.insert(candidate.clone()) {
        return candidate;
    }
    let mut suffix = 2_usize;
    loop {
        let numbered = format!("{slug}-{suffix}.{extension}");
        if used_filenames.insert(numbered.clone()) {
            return numbered;
        }
        suffix = suffix.saturating_add(1_usize);
    }
}

fn copy_referenced_script(
    check_map: &mut Map<String, Value>,
    filename: &str,
    source_dir: &Path,
    destination_scripts_dir: &Path,
) -> io::Result<()> {
    let direct_source = source_dir.join(filename);
    let nested_source = source_dir.join(SCRIPTS_DIR).join(filename);
    let source_path = if direct_source.is_file() {
        Some(direct_source)
    } else if nested_source.is_file() {
        Some(nested_source)
    } else {
        None
    };
    let destination_path = destination_scripts_dir.join(filename);
    if let Some(script_source) = source_path {
        let bytes = fs::read(&script_source)?;
        if !destination_path.exists() {
            fs::write(&destination_path, &bytes)?;
        }
        if read_non_empty_string(check_map, "scriptHash").is_none() {
            let _prior =
                check_map.insert("scriptHash".into(), Value::String(hash_sha256_hex(&bytes)));
        }
        return Ok(());
    }
    if !destination_path.exists()
        && let Some(inline_test) = read_non_empty_string(check_map, "test")
    {
        fs::write(&destination_path, inline_test.as_bytes())?;
        if read_non_empty_string(check_map, "scriptHash").is_none() {
            let _prior = check_map.insert(
                "scriptHash".into(),
                Value::String(hash_sha256_hex(inline_test.as_bytes())),
            );
        }
    }
    Ok(())
}

fn format_check_slug(check_id: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for character in check_id.chars() {
        if character.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character.to_ascii_lowercase());
            pending_dash = false;
        } else {
            pending_dash = true;
        }
    }
    let bounded: String = slug.chars().take(MAX_SLUG_CHARS).collect();
    let trimmed = bounded.trim_matches('-');
    if trimmed.is_empty() {
        String::from("check")
    } else {
        trimmed.to_owned()
    }
}

fn hash_sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn read_mutable_checks<'value>(
    pin_value: &'value mut Value,
    source_lock: &Path,
) -> io::Result<&'value mut Vec<Value>> {
    pin_value
        .get_mut("bundle")
        .and_then(Value::as_object_mut)
        .and_then(|bundle| bundle.get_mut("checks"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "behavior pin {:?} is missing bundle.checks",
                    source_lock.display()
                ),
            )
        })
}

fn read_non_empty_string(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn resolve_runtime_extension(runtime: &str) -> &'static str {
    match runtime {
        "go" => "go",
        "node" => "mjs",
        "python" => "py",
        "rust" => "rs",
        _ => "txt",
    }
}

fn validate_pin_protocol(pin_value: &Value, source_lock: &Path) -> io::Result<()> {
    let protocol = pin_value.get("protocol").and_then(Value::as_u64);
    if protocol == Some(SUPPORTED_PIN_PROTOCOL) {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "unsupported behavior pin protocol {protocol:?} in {:?}",
            source_lock.display()
        ),
    ))
}

fn validate_script_filename(raw: &str) -> io::Result<&str> {
    let valid_chars = raw
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && raw.len() <= MAX_SCRIPT_NAME_CHARS
        && !raw.contains("..")
        && raw.chars().all(|character| {
            character.is_ascii_alphanumeric() || ALLOWED_SCRIPT_PUNCTUATION.contains(&character)
        });
    if !valid_chars {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid behavior script filename: {raw:?}"),
        ));
    }
    let mut components = Path::new(raw).components();
    if !matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    ) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("behavior script filename must be a single path segment: {raw:?}"),
        ));
    }
    Ok(raw)
}

fn write_check_entry(
    check: &mut Value,
    source_dir: &Path,
    destination_scripts_dir: &Path,
    used_filenames: &mut BTreeSet<String>,
) -> io::Result<()> {
    let check_map = check.as_object_mut().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "behavior pin check entry must be a JSON object",
        )
    })?;
    if let Some(script_file) = read_non_empty_string(check_map, "scriptFile") {
        let filename = validate_script_filename(&script_file)?;
        let _recorded = used_filenames.insert(filename.to_owned());
        copy_referenced_script(check_map, filename, source_dir, destination_scripts_dir)?;
        return Ok(());
    }
    if read_non_empty_string(check_map, "test").is_some() {
        write_legacy_script(check_map, destination_scripts_dir, used_filenames)?;
    }
    Ok(())
}

fn write_legacy_script(
    check_map: &mut Map<String, Value>,
    destination_scripts_dir: &Path,
    used_filenames: &mut BTreeSet<String>,
) -> io::Result<()> {
    let Some(inline_test) = read_non_empty_string(check_map, "test") else {
        return Ok(());
    };
    let check_id = read_non_empty_string(check_map, "id").unwrap_or_else(|| String::from("check"));
    let runtime = read_non_empty_string(check_map, "runtime").unwrap_or_default();
    let filename = allocate_script_filename(&check_id, &runtime, used_filenames);
    let destination_path = destination_scripts_dir.join(&filename);
    if !destination_path.exists() {
        fs::write(&destination_path, inline_test.as_bytes())?;
    }
    let hash = hash_sha256_hex(inline_test.as_bytes());
    let _prior_file = check_map.insert("scriptFile".into(), Value::String(filename));
    let _prior_hash = check_map.insert("scriptHash".into(), Value::String(hash));
    let _removed_test = check_map.remove("test");
    Ok(())
}
