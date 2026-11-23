use std::{fs, path::Path, process::Output};

use anyhow::{Result, ensure};
use serde_json::Value;

use super::{build_tenet::build_tenet, run_tenet::run_tenet};
use crate::{create_catalog_fixture, parse_success_json, write_pin_file};

fn run_without_openrouter(root: &Path, args: &[&str]) -> Result<Output> {
    Ok(build_tenet()
        .arg("--root")
        .arg(root)
        .args(args)
        .env_remove("OPENROUTER_API_KEY")
        .output()?)
}

fn create_offline_demo_repo(root: &Path) -> Result<()> {
    fs::create_dir_all(root.join("src"))?;
    fs::write(
        root.join("src/choose.mjs"),
        "export const choose = (v) => (v === 0 ? 0 : v || 7);\n",
    )?;
    let contract_dir = root.join(".contracts/001-demo");
    fs::create_dir_all(contract_dir.join("scripts"))?;
    fs::write(
        contract_dir.join("CONTRACT.md"),
        "---\nname: demo\nmessage: Preserve explicit zero.\npaths:\n  - \
         'src/choose.mjs'\n---\n\n## Rules\n\nPreserve explicit zero.\n",
    )?;
    fs::write(
        contract_dir.join("scripts/keeps-zero.mjs"),
        "import { choose } from './src/choose.mjs';\nconsole.log(JSON.stringify(choose(0)));\n",
    )?;
    let lock = serde_json::json!({
        "protocol": 1_u64,
        "contractHash": "demo",
        "shape": "s1",
        "bundle": {
            "checks": [{
                "id": "keeps-zero",
                "promise": "Preserve explicit zero.",
                "subject": "src/choose.mjs",
                "runtime": "node",
                "scriptFile": "keeps-zero.mjs",
                "expected": 0_u64,
                "good": "export const choose = (v) => v ?? 7;",
                "defect": "export const choose = (v) => v || 7;",
                "refactor": "export const choose = (v) => (v == null ? 7 : v);"
            }],
            "open": []
        }
    });
    fs::write(
        contract_dir.join("scripts/.lock.json"),
        serde_json::to_string_pretty(&lock)?,
    )?;
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests report failures through assertions."
)]
fn scan_scaffolds_starter_without_api_key() -> Result<()> {
    let starter_project = tempfile::tempdir()?;
    let starter = run_tenet(
        starter_project.path(),
        &["scan", "--starter", "scan-guards", "--json"],
    )?;
    ensure!(
        starter.status.success(),
        "{:?}",
        String::from_utf8_lossy(&starter.stderr)
    );
    let starter_doc: Value = serde_json::from_slice(&starter.stdout)?;
    assert_eq!(starter_doc.get("created"), Some(&Value::Bool(true)));
    assert!(
        starter_project
            .path()
            .join(".contracts/001-scan-guards/CONTRACT.md")
            .is_file()
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests report failures through assertions."
)]
fn scan_previews_and_applies_local_catalog_without_api_key() -> Result<()> {
    let (catalog_project, _catalog, lib_arg) = create_catalog_fixture()?;
    let preview = run_without_openrouter(
        catalog_project.path(),
        &["scan", "--library", &lib_arg, "--json"],
    )?;
    let preview_doc = parse_success_json(&preview)?;
    assert_eq!(
        preview_doc.get("mode").and_then(Value::as_str),
        Some("preview")
    );
    assert_eq!(preview_doc.get("applied"), Some(&Value::Bool(false)));
    assert!(!catalog_project.path().join(".contracts").exists());

    let applied = run_without_openrouter(
        catalog_project.path(),
        &["scan", "--library", &lib_arg, "--apply", "--json"],
    )?;
    let applied_doc = parse_success_json(&applied)?;
    assert_eq!(
        applied_doc.get("mode").and_then(Value::as_str),
        Some("apply")
    );
    assert_eq!(applied_doc.get("applied"), Some(&Value::Bool(true)));
    assert!(catalog_project.path().join(".tenet.lock").is_file());
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests report failures through assertions."
)]
fn status_reports_offline_coverage_before_any_model_check() -> Result<()> {
    let project = tempfile::tempdir()?;
    let root = project.path();
    fs::create_dir_all(root.join("src"))?;
    fs::create_dir_all(root.join("lib"))?;
    fs::write(root.join("src/core.mjs"), "export const val = 1;\n")?;
    fs::write(root.join("lib/helper.mjs"), "export const h = 2;\n")?;
    let contract_dir = root.join(".contracts/001-demo");
    fs::create_dir_all(&contract_dir)?;
    fs::write(
        contract_dir.join("CONTRACT.md"),
        "---\nname: demo\nmessage: Keep core valid.\npaths:\n  - 'src/**'\n---\n\n## \
         Rules\n\nKeep core valid.\n",
    )?;
    write_pin_file(
        &contract_dir.join("scripts/.lock.json"),
        &serde_json::json!([{
            "id": "core-check",
            "promise": "Keep core valid.",
            "runtime": "node",
            "scriptFile": "core-check.mjs",
            "subject": "src/core.mjs"
        }]),
    )?;

    let text = run_without_openrouter(root, &["status"])?;
    assert_eq!(text.status.code(), Some(4_i32));
    let stdout = String::from_utf8(text.stdout)?;
    assert!(stdout.contains("Coverage"), "{stdout:?}");
    assert!(
        stdout.contains("1/2 source files governed by 1 contract"),
        "{stdout:?}"
    );
    assert!(stdout.contains("lib"), "{stdout:?}");

    let raw_json = run_without_openrouter(root, &["status", "--json"])?;
    let doc: Value = serde_json::from_slice(&raw_json.stdout)?;
    assert_eq!(raw_json.status.code(), Some(4_i32));
    assert_eq!(
        doc.pointer("/coverage/total_files"),
        Some(&Value::from(2_u64))
    );
    assert_eq!(
        doc.pointer("/coverage/governed_files"),
        Some(&Value::from(1_u64))
    );
    assert_eq!(
        doc.pointer("/coverage/uncovered_files"),
        Some(&Value::from(1_u64))
    );
    assert_eq!(
        doc.pointer("/coverage/contracts/0/script_count"),
        Some(&Value::from(1_u64))
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests report failures through assertions."
)]
fn check_offline_replays_pinned_scripts_without_api_key() -> Result<()> {
    let project = tempfile::tempdir()?;
    let root = project.path();
    create_offline_demo_repo(root)?;

    let passing = run_without_openrouter(root, &["check", "--offline", "--json"])?;
    let pass_doc: Value = serde_json::from_slice(&passing.stdout)?;
    assert_eq!(
        pass_doc.get("mode").and_then(Value::as_str),
        Some("offline-check")
    );
    let pass_code = passing.status.code();
    assert!(
        matches!(pass_code, Some(0_i32 | 3_i32)),
        "expected exit 0 or 3, got {pass_code:?}: {:?}",
        String::from_utf8_lossy(&passing.stdout)
    );
    if pass_code == Some(0_i32) {
        assert_eq!(
            pass_doc.pointer("/summary/behavior_passed"),
            Some(&Value::from(1_u64))
        );
        fs::write(
            root.join("src/choose.mjs"),
            "export const choose = (v) => v || 7;\n",
        )?;
        let failing = run_without_openrouter(root, &["check", "--offline", "--json"])?;
        assert_eq!(
            failing.status.code(),
            Some(1_i32),
            "{:?}",
            String::from_utf8_lossy(&failing.stdout)
        );
        let fail_doc: Value = serde_json::from_slice(&failing.stdout)?;
        assert_eq!(
            fail_doc.pointer("/summary/behavior_failed"),
            Some(&Value::from(1_u64))
        );
    }
    Ok(())
}
