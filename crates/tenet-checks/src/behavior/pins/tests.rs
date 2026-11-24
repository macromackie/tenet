use std::fs;

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use super::{
    LEGACY_LOCK_FILE,
    PinBundleState,
    PinSummary,
    PinnedCheck,
    SCRIPTS_DIR,
    SCRIPTS_LOCK_FILE,
    copy_staged_pin_bundle,
    find_pin_path,
    read_pin_summary,
};

fn build_staged_pin_fixture(script_a: &str) -> Value {
    json!({
        "protocol": 1_u64,
        "shape": "shape-1",
        "bundle": {
            "checks": [
                {
                    "id": "check-a",
                    "promise": "Check A",
                    "runtime": "node",
                    "subject": "src/a.ts",
                    "scriptFile": "check-a.mjs",
                    "scriptHash": format!("{:x}", Sha256::digest(script_a.as_bytes()))
                },
                {
                    "id": "check-b",
                    "promise": "Check B",
                    "runtime": "python",
                    "subject": "src/b.py",
                    "scriptFile": "check-b.py"
                }
            ],
            "open": []
        }
    })
}

fn build_legacy_pin_fixture(inline_code: &str) -> Value {
    json!({
        "protocol": 1_u64,
        "shape": "shape-legacy",
        "contractHash": "hash-123",
        "bundle": {
            "checks": [
                {
                    "id": "Reject Empty / Nil",
                    "promise": "Reject empty inputs",
                    "runtime": "node",
                    "subject": "src/input.ts",
                    "defect": "accepts empty string",
                    "expected": { "ok": true },
                    "good": "throws on empty string",
                    "refactor": "extract validator",
                    "test": inline_code
                }
            ],
            "open": []
        }
    })
}

#[test]
fn find_pin_path_prefers_scripts_lock_over_legacy_lock() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let contract_dir = temp.path();

    anyhow::ensure!(find_pin_path(contract_dir).is_none());

    let legacy_path = contract_dir.join(LEGACY_LOCK_FILE);
    fs::write(&legacy_path, "{}")?;
    anyhow::ensure!(find_pin_path(contract_dir) == Some(legacy_path));

    let scripts_dir = contract_dir.join(SCRIPTS_DIR);
    fs::create_dir_all(&scripts_dir)?;
    let unified_path = contract_dir.join(SCRIPTS_LOCK_FILE);
    fs::write(&unified_path, "{}")?;
    anyhow::ensure!(find_pin_path(contract_dir) == Some(unified_path));

    Ok(())
}

#[test]
fn read_pin_summary_parses_valid_pin_and_rejects_malformed_or_unsupported() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let valid_path = temp.path().join(".lock.json");
    let valid_pin = json!({
        "protocol": 1_u64,
        "shape": "shape-abc",
        "bundle": {
            "checks": [{
                "id": "preserve-zero",
                "promise": "Preserve explicit zero values",
                "runtime": "node",
                "subject": "src/options.ts",
                "scriptFile": "preserve-zero.mjs",
                "scriptHash": "deadbeef"
            }],
            "open": []
        }
    });
    fs::write(&valid_path, serde_json::to_vec(&valid_pin)?)?;

    let summary = read_pin_summary(&valid_path).map_err(anyhow::Error::msg)?;
    let expected = PinSummary {
        check_count: 1_usize,
        checks: vec![PinnedCheck {
            id: String::from("preserve-zero"),
            promise: String::from("Preserve explicit zero values"),
            runtime: String::from("node"),
            script_file: Some(String::from("preserve-zero.mjs")),
            script_hash: Some(String::from("deadbeef")),
            subject: String::from("src/options.ts"),
        }],
        lock_path: valid_path,
        protocol: 1_u64,
    };
    anyhow::ensure!(summary == expected, "unexpected pin summary: {summary:?}");

    let malformed_path = temp.path().join("malformed.json");
    fs::write(&malformed_path, "{ not valid json")?;
    anyhow::ensure!(read_pin_summary(&malformed_path).is_err());

    let unsupported_path = temp.path().join("unsupported.json");
    fs::write(
        &unsupported_path,
        serde_json::to_vec(&json!({ "protocol": 2_u64, "bundle": { "checks": [] } }))?,
    )?;
    anyhow::ensure!(read_pin_summary(&unsupported_path).is_err());
    Ok(())
}

#[test]
fn copy_staged_pin_bundle_copies_sibling_scripts_and_preserves_hand_edits() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let missing_lock = temp.path().join("missing/.lock.json");
    let destination = temp.path().join("contract");
    anyhow::ensure!(
        copy_staged_pin_bundle(&missing_lock, &destination)? == PinBundleState::Missing
    );

    let staged_dir = temp.path().join("staged/scripts");
    fs::create_dir_all(&staged_dir)?;
    let script_a = "console.log(JSON.stringify({ ok: true }));\n";
    let script_b = "print('{\"ok\": true}')\n";
    fs::write(staged_dir.join("check-a.mjs"), script_a)?;
    fs::write(staged_dir.join("check-b.py"), script_b)?;

    let staged_lock = staged_dir.join(".lock.json");
    fs::write(
        &staged_lock,
        serde_json::to_vec_pretty(&build_staged_pin_fixture(script_a))?,
    )?;

    let dest_scripts = destination.join(SCRIPTS_DIR);
    fs::create_dir_all(&dest_scripts)?;
    let hand_edited = "// custom local edit\n";
    fs::write(dest_scripts.join("check-a.mjs"), hand_edited)?;

    anyhow::ensure!(copy_staged_pin_bundle(&staged_lock, &destination)? == PinBundleState::Written);
    anyhow::ensure!(fs::read_to_string(dest_scripts.join("check-a.mjs"))? == hand_edited);
    anyhow::ensure!(fs::read_to_string(dest_scripts.join("check-b.py"))? == script_b);

    let copied_summary =
        read_pin_summary(&destination.join(SCRIPTS_LOCK_FILE)).map_err(anyhow::Error::msg)?;
    anyhow::ensure!(copied_summary.check_count == 2_usize);
    let expected_hash_b = format!("{:x}", Sha256::digest(script_b.as_bytes()));
    let check_b = copied_summary
        .checks
        .get(1)
        .ok_or_else(|| anyhow::anyhow!("missing second check"))?;
    anyhow::ensure!(check_b.script_hash.as_deref() == Some(expected_hash_b.as_str()));
    Ok(())
}

#[test]
fn copy_staged_pin_bundle_migrates_legacy_inline_pin() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let legacy_lock = temp.path().join("legacy/agent-checks.lock.json");
    let legacy_parent = legacy_lock
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing legacy parent"))?;
    fs::create_dir_all(legacy_parent)?;

    let inline_code = "console.log(JSON.stringify({ ok: true }));\n";
    fs::write(
        &legacy_lock,
        serde_json::to_vec_pretty(&build_legacy_pin_fixture(inline_code))?,
    )?;

    let destination = temp.path().join("contract");
    anyhow::ensure!(copy_staged_pin_bundle(&legacy_lock, &destination)? == PinBundleState::Written);

    let migrated_script = destination.join("scripts/reject-empty-nil.mjs");
    anyhow::ensure!(fs::read_to_string(&migrated_script)? == inline_code);

    let migrated_lock = destination.join(SCRIPTS_LOCK_FILE);
    let parsed: Value = serde_json::from_str(&fs::read_to_string(&migrated_lock)?)?;
    let first_check = parsed
        .pointer("/bundle/checks/0")
        .ok_or_else(|| anyhow::anyhow!("missing migrated check"))?;
    let expected_hash = format!("{:x}", Sha256::digest(inline_code.as_bytes()));
    let expected_controls = serde_json::to_string(&[
        "throws on empty string",
        "accepts empty string",
        "extract validator",
    ])?;
    let expected_control_hash = format!("{:x}", Sha256::digest(expected_controls.as_bytes()));
    anyhow::ensure!(first_check.get("test").is_none());
    anyhow::ensure!(first_check.get("good").is_none());
    anyhow::ensure!(first_check.get("defect").is_none());
    anyhow::ensure!(first_check.get("refactor").is_none());
    anyhow::ensure!(first_check.get("scriptFile") == Some(&json!("reject-empty-nil.mjs")));
    anyhow::ensure!(first_check.get("scriptHash") == Some(&json!(expected_hash)));
    anyhow::ensure!(first_check.get("controlHash") == Some(&json!(expected_control_hash)));
    Ok(())
}

#[test]
fn copy_staged_pin_bundle_rejects_escaping_script_file() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let destination = temp.path().join("contract");
    let escaping_lock = temp.path().join("escaping.lock.json");
    fs::write(
        &escaping_lock,
        serde_json::to_vec(&json!({
            "protocol": 1_u64,
            "bundle": {
                "checks": [{
                    "id": "escape",
                    "promise": "escape",
                    "runtime": "node",
                    "subject": "src/a.ts",
                    "scriptFile": "../escape.mjs"
                }]
            }
        }))?,
    )?;
    anyhow::ensure!(copy_staged_pin_bundle(&escaping_lock, &destination).is_err());
    Ok(())
}
