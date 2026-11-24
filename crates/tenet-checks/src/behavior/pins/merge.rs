use alloc::collections::BTreeSet;
use core::mem;
use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

use serde_json::{Map, Value};

use super::{
    PinBundleState,
    SCRIPTS_DIR,
    scripts::{
        FilenamePolicy,
        MAX_SLUG_CHARS,
        SUPPORTED_PIN_PROTOCOL,
        format_check_slug,
        hash_sha256_hex,
        read_mutable_checks,
        read_non_empty_string,
        read_pin_json,
        write_check_entry,
        write_pretty_pin_lock,
    },
};

#[derive(Default)]
struct MergeState {
    checks: Vec<Value>,
    loaded_count: usize,
    open: Vec<Value>,
    seen_open_items: BTreeSet<String>,
    shapes: Vec<String>,
    used_filenames: BTreeSet<String>,
    used_ids: BTreeSet<String>,
}

pub(super) fn merge_staged_pin_bundles(
    source_locks: &[PathBuf],
    destination_contract_dir: &Path,
    contract_hash: &str,
) -> io::Result<PinBundleState> {
    let mut state = MergeState::default();
    let destination_scripts_dir = destination_contract_dir.join(SCRIPTS_DIR);
    for source_lock in source_locks {
        merge_one_lock(&mut state, source_lock, &destination_scripts_dir)?;
    }
    if state.loaded_count == 0_usize {
        return Ok(PinBundleState::Missing);
    }
    let combined_shape = compute_combined_shape(&mut state.shapes)?;
    let merged = build_merged_pin(state.checks, state.open, contract_hash, combined_shape);
    write_pretty_pin_lock(destination_contract_dir, &merged)?;
    Ok(PinBundleState::Written)
}

fn allocate_unique_check_id(raw_id: &str, used_ids: &mut BTreeSet<String>) -> String {
    if !raw_id.is_empty() && used_ids.insert(raw_id.to_owned()) {
        return raw_id.to_owned();
    }
    let slug = format_check_slug(raw_id);
    if used_ids.insert(slug.clone()) {
        return slug;
    }
    let mut suffix = 2_usize;
    loop {
        let tag = format!("-{suffix}");
        let prefix_budget = MAX_SLUG_CHARS.saturating_sub(tag.len());
        let prefix: String = slug.chars().take(prefix_budget).collect();
        let trimmed = prefix.trim_end_matches('-');
        let base = if trimmed.is_empty() { "check" } else { trimmed };
        let candidate = format!("{base}{tag}");
        if used_ids.insert(candidate.clone()) {
            return candidate;
        }
        suffix = suffix.saturating_add(1_usize);
    }
}

fn build_merged_pin(
    checks: Vec<Value>,
    open: Vec<Value>,
    contract_hash: &str,
    combined_shape: String,
) -> Value {
    let mut bundle = Map::new();
    let _checks = bundle.insert("checks".into(), Value::Array(checks));
    let _open = bundle.insert("open".into(), Value::Array(open));
    let mut merged = Map::new();
    let _bundle = merged.insert("bundle".into(), Value::Object(bundle));
    if !contract_hash.is_empty() {
        let _hash = merged.insert(
            "contractHash".into(),
            Value::String(contract_hash.to_owned()),
        );
    }
    let _protocol = merged.insert("protocol".into(), Value::from(SUPPORTED_PIN_PROTOCOL));
    let _shape = merged.insert("shape".into(), Value::String(combined_shape));
    Value::Object(merged)
}

fn collect_open_items(state: &mut MergeState, pin_value: &Value) {
    let Some(open_entries) = pin_value
        .get("bundle")
        .and_then(|bundle| bundle.get("open"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for item in open_entries {
        if let Some(text) = item.as_str()
            && !text.is_empty()
            && state.seen_open_items.insert(text.to_owned())
        {
            state.open.push(Value::String(text.to_owned()));
        }
    }
}

fn compute_combined_shape(shapes: &mut [String]) -> io::Result<String> {
    shapes.sort();
    let encoded = serde_json::to_string(shapes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(hash_sha256_hex(encoded.as_bytes()))
}

fn deduplicate_check_id(check: &mut Value, used_ids: &mut BTreeSet<String>) -> io::Result<()> {
    let check_map = check.as_object_mut().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "behavior pin check entry must be a JSON object",
        )
    })?;
    let raw_id = read_non_empty_string(check_map, "id").unwrap_or_else(|| String::from("check"));
    let unique_id = allocate_unique_check_id(&raw_id, used_ids);
    let _prior = check_map.insert("id".into(), Value::String(unique_id));
    Ok(())
}

fn merge_one_lock(
    state: &mut MergeState,
    source_lock: &Path,
    destination_scripts_dir: &Path,
) -> io::Result<()> {
    let Some(mut pin_value) = read_pin_json(source_lock)? else {
        return Ok(());
    };
    if state.loaded_count == 0_usize {
        fs::create_dir_all(destination_scripts_dir)?;
    }
    state.loaded_count = state.loaded_count.saturating_add(1_usize);
    if let Some(shape) = pin_value
        .get("shape")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        state.shapes.push(shape.to_owned());
    }
    collect_open_items(state, &pin_value);
    let source_dir = source_lock.parent().unwrap_or_else(|| Path::new("."));
    let checks = mem::take(read_mutable_checks(&mut pin_value, source_lock)?);
    for mut check in checks {
        deduplicate_check_id(&mut check, &mut state.used_ids)?;
        write_check_entry(
            &mut check,
            source_dir,
            destination_scripts_dir,
            &mut state.used_filenames,
            FilenamePolicy::Deduplicate,
        )?;
        state.checks.push(check);
    }
    Ok(())
}
