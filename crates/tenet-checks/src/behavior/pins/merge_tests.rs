use std::{
    fs,
    path::{Path, PathBuf},
};

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use super::{
    PinBundleState,
    SCRIPTS_DIR,
    SCRIPTS_LOCK_FILE,
    merge_staged_pin_bundles,
    read_pin_summary,
};

fn build_pin_one(script_code: &str) -> Value {
    json!({
        "protocol": 1_u64,
        "shape": "shape-zeta",
        "bundle": {
            "checks": [{
                "id": "keeps-zero",
                "promise": "First contract keeps zero",
                "runtime": "node",
                "subject": "src/a.ts",
                "expected": 0_i64,
                "good": "good-a",
                "defect": "defect-a",
                "refactor": "refactor-a",
                "scriptFile": "keeps-zero.mjs",
                "scriptHash": format!("{:x}", Sha256::digest(script_code.as_bytes()))
            }],
            "open": ["shared-open", "open-one"]
        }
    })
}

fn build_pin_two() -> Value {
    json!({
        "protocol": 1_u64,
        "shape": "shape-alpha",
        "bundle": {
            "checks": [
                {
                    "id": "keeps-zero",
                    "promise": "Second contract also checks zero",
                    "runtime": "node",
                    "subject": "src/b.ts",
                    "expected": 1_i64,
                    "controlHash": "preset-control-hash",
                    "good": "ignored-when-preset",
                    "scriptFile": "keeps-zero.mjs"
                },
                {
                    "id": "py-check",
                    "promise": "Python inline check",
                    "runtime": "python",
                    "subject": "src/c.py",
                    "expected": true,
                    "good": "good-py",
                    "defect": "defect-py",
                    "refactor": "refactor-py",
                    "test": "print('true')\n"
                }
            ],
            "open": ["shared-open", "open-two"]
        }
    })
}

fn write_two_staged_pins(root: &Path) -> anyhow::Result<(PathBuf, PathBuf)> {
    let stage_one = root.join("stage-1/scripts");
    let stage_two = root.join("stage-2/scripts");
    fs::create_dir_all(&stage_one)?;
    fs::create_dir_all(&stage_two)?;
    let script_one = "console.log(0);\n";
    let script_two = "console.log(1);\n";
    fs::write(stage_one.join("keeps-zero.mjs"), script_one)?;
    fs::write(stage_two.join("keeps-zero.mjs"), script_two)?;
    let lock_one = stage_one.join(".lock.json");
    let lock_two = stage_two.join(".lock.json");
    fs::write(
        &lock_one,
        serde_json::to_vec_pretty(&build_pin_one(script_one))?,
    )?;
    fs::write(&lock_two, serde_json::to_vec_pretty(&build_pin_two())?)?;
    Ok((lock_one, lock_two))
}

fn verify_slim_merged_json(parsed: &Value, expected_shape: &str) -> anyhow::Result<()> {
    anyhow::ensure!(parsed.get("contractHash") == Some(&json!("contract-hash-42")));
    anyhow::ensure!(parsed.get("shape") == Some(&json!(expected_shape)));
    anyhow::ensure!(
        parsed.pointer("/bundle/open") == Some(&json!(["shared-open", "open-one", "open-two"]))
    );
    let first_controls = serde_json::to_string(&["good-a", "defect-a", "refactor-a"])?;
    let expected_first_hash = format!("{:x}", Sha256::digest(first_controls.as_bytes()));
    let checks = parsed
        .pointer("/bundle/checks")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("missing merged checks"))?;
    for entry in checks {
        anyhow::ensure!(entry.get("good").is_none());
        anyhow::ensure!(entry.get("defect").is_none());
        anyhow::ensure!(entry.get("refactor").is_none());
        anyhow::ensure!(entry.get("test").is_none());
    }
    anyhow::ensure!(
        checks.first().and_then(|item| item.get("controlHash"))
            == Some(&json!(expected_first_hash))
    );
    anyhow::ensure!(
        checks.get(1).and_then(|item| item.get("controlHash"))
            == Some(&json!("preset-control-hash"))
    );
    Ok(())
}

#[test]
fn merge_staged_pin_bundles_returns_missing_when_no_locks_exist() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let dest = temp.path().join("empty-contract");
    let missing = vec![temp.path().join("nope-1/.lock.json")];
    anyhow::ensure!(merge_staged_pin_bundles(&[], &dest, "hash-empty")? == PinBundleState::Missing);
    anyhow::ensure!(
        merge_staged_pin_bundles(&missing, &dest, "hash-empty")? == PinBundleState::Missing
    );
    anyhow::ensure!(!dest.join(SCRIPTS_LOCK_FILE).exists());
    Ok(())
}

#[test]
fn merge_staged_pin_bundles_deduplicates_ids_filenames_strips_controls_and_preserves_edits()
-> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let (lock_one, lock_two) = write_two_staged_pins(temp.path())?;
    let dest = temp.path().join("merged-contract");
    let dest_scripts = dest.join(SCRIPTS_DIR);
    fs::create_dir_all(&dest_scripts)?;
    let hand_edited = "// local hand edit\n";
    fs::write(dest_scripts.join("keeps-zero.mjs"), hand_edited)?;

    let sources = vec![
        lock_one.clone(),
        temp.path().join("missing.lock.json"),
        lock_two.clone(),
    ];
    let state = merge_staged_pin_bundles(&sources, &dest, "contract-hash-42")?;
    anyhow::ensure!(state == PinBundleState::Written);
    anyhow::ensure!(fs::read_to_string(dest_scripts.join("keeps-zero.mjs"))? == hand_edited);
    anyhow::ensure!(
        fs::read_to_string(dest_scripts.join("keeps-zero-2.mjs"))? == "console.log(1);\n"
    );
    anyhow::ensure!(fs::read_to_string(dest_scripts.join("py-check.py"))? == "print('true')\n");

    let summary = read_pin_summary(&dest.join(SCRIPTS_LOCK_FILE)).map_err(anyhow::Error::msg)?;
    let ids: Vec<&str> = summary.checks.iter().map(|item| item.id.as_str()).collect();
    let files: Vec<Option<&str>> = summary
        .checks
        .iter()
        .map(|item| item.script_file.as_deref())
        .collect();
    anyhow::ensure!(ids == vec!["keeps-zero", "keeps-zero-2", "py-check"]);
    anyhow::ensure!(
        files
            == vec![
                Some("keeps-zero.mjs"),
                Some("keeps-zero-2.mjs"),
                Some("py-check.py")
            ]
    );

    let expected_shapes = serde_json::to_string(&["shape-alpha", "shape-zeta"])?;
    let expected_shape = format!("{:x}", Sha256::digest(expected_shapes.as_bytes()));
    let parsed: Value = serde_json::from_str(&fs::read_to_string(dest.join(SCRIPTS_LOCK_FILE))?)?;
    verify_slim_merged_json(&parsed, &expected_shape)?;

    let reversed_dest = temp.path().join("reversed-contract");
    let reversed_state =
        merge_staged_pin_bundles(&[lock_two, lock_one], &reversed_dest, "contract-hash-42")?;
    anyhow::ensure!(reversed_state == PinBundleState::Written);
    let reversed_parsed: Value =
        serde_json::from_str(&fs::read_to_string(reversed_dest.join(SCRIPTS_LOCK_FILE))?)?;
    anyhow::ensure!(reversed_parsed.get("shape") == Some(&json!(expected_shape)));
    Ok(())
}
