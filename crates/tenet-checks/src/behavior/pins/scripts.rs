use alloc::collections::BTreeSet;
use std::{
    fs,
    io,
    path::{Component, Path},
};

use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

use super::{SCRIPTS_DIR, SCRIPTS_LOCK_FILE};

pub(super) const MAX_SLUG_CHARS: usize = 64_usize;
pub(super) const SUPPORTED_PIN_PROTOCOL: u64 = 1_u64;

const ALLOWED_SCRIPT_PUNCTUATION: &[char] = &['.', '_', '-'];
const MAX_SCRIPT_NAME_CHARS: usize = 128_usize;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum FilenamePolicy {
    Deduplicate,
    Keep,
}

pub(super) fn format_check_slug(check_id: &str) -> String {
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

pub(super) fn hash_sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn read_mutable_checks<'value>(
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

pub(super) fn read_non_empty_string(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(super) fn read_pin_json(source_lock: &Path) -> io::Result<Option<Value>> {
    let raw_text = match fs::read_to_string(source_lock) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let pin_value: Value = serde_json::from_str(&raw_text).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "invalid behavior pin JSON {:?}: {error:#}",
                source_lock.display()
            ),
        )
    })?;
    validate_pin_protocol(&pin_value, source_lock)?;
    Ok(Some(pin_value))
}

pub(super) fn write_check_entry(
    check: &mut Value,
    source_dir: &Path,
    destination_scripts_dir: &Path,
    used_filenames: &mut BTreeSet<String>,
    policy: FilenamePolicy,
) -> io::Result<()> {
    let check_map = check.as_object_mut().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "behavior pin check entry must be a JSON object",
        )
    })?;
    if let Some(script_file) = read_non_empty_string(check_map, "scriptFile") {
        let source_filename = validate_script_filename(&script_file)?.to_owned();
        let dest_filename =
            resolve_destination_filename(check_map, &source_filename, used_filenames, policy);
        copy_referenced_script(
            check_map,
            &source_filename,
            &dest_filename,
            source_dir,
            destination_scripts_dir,
        )?;
        let _removed_test = check_map.remove("test");
    } else if read_non_empty_string(check_map, "test").is_some() {
        write_legacy_script(check_map, destination_scripts_dir, used_filenames)?;
    } else {
        // Native `testFile` or scriptless check; only control stripping applies.
    }
    strip_check_controls(check_map)
}

pub(super) fn write_pretty_pin_lock(
    destination_contract_dir: &Path,
    pin_value: &Value,
) -> io::Result<()> {
    let mut encoded = serde_json::to_string_pretty(pin_value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    encoded.push('\n');
    fs::write(destination_contract_dir.join(SCRIPTS_LOCK_FILE), encoded)
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
    source_filename: &str,
    destination_filename: &str,
    source_dir: &Path,
    destination_scripts_dir: &Path,
) -> io::Result<()> {
    let direct_source = source_dir.join(source_filename);
    let nested_source = source_dir.join(SCRIPTS_DIR).join(source_filename);
    let source_path = if direct_source.is_file() {
        Some(direct_source)
    } else if nested_source.is_file() {
        Some(nested_source)
    } else {
        None
    };
    let destination_path = destination_scripts_dir.join(destination_filename);
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

fn resolve_destination_filename(
    check_map: &mut Map<String, Value>,
    source_filename: &str,
    used_filenames: &mut BTreeSet<String>,
    policy: FilenamePolicy,
) -> String {
    if used_filenames.insert(source_filename.to_owned()) || policy == FilenamePolicy::Keep {
        return source_filename.to_owned();
    }
    let check_id = read_non_empty_string(check_map, "id").unwrap_or_else(|| String::from("check"));
    let runtime = read_non_empty_string(check_map, "runtime").unwrap_or_default();
    let allocated = allocate_script_filename(&check_id, &runtime, used_filenames);
    let _prior = check_map.insert("scriptFile".into(), Value::String(allocated.clone()));
    allocated
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

fn strip_check_controls(check_map: &mut Map<String, Value>) -> io::Result<()> {
    let existing_hash = read_non_empty_string(check_map, "controlHash");
    let good = remove_string_field(check_map, "good");
    let defect = remove_string_field(check_map, "defect");
    let refactor = remove_string_field(check_map, "refactor");
    if existing_hash.is_none() && (!good.is_empty() || !defect.is_empty() || !refactor.is_empty()) {
        let encoded = serde_json::to_string(&[&good, &defect, &refactor])
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let hash = hash_sha256_hex(encoded.as_bytes());
        let _prior = check_map.insert("controlHash".into(), Value::String(hash));
    }
    Ok(())
}

fn remove_string_field(check_map: &mut Map<String, Value>, key: &str) -> String {
    check_map
        .remove(key)
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_default()
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
