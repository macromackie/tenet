use std::path::Path;

use anyhow::Result;
use serde_json::{Value, json};

use super::{
    fixture::{run_git, run_tenet},
    provider::Provider,
};
use crate::{commit::commit, write_file::write_file};

fn create_failing_check_repo(root: &Path) -> Result<()> {
    write_file(
        root,
        ".contracts/001-rule/CONTRACT.md",
        "---\nname: rule\nmessage: Return one from val().\npaths: ['calc.py']\nobligations: \
         [val]\n---\n\n## Rules\n\n`val()` in `calc.py` must return `1`.\n",
    )?;
    write_file(root, "calc.py", "def val():\n    return 0\n")?;
    write_file(
        root,
        "checks/val.sh",
        "if grep -q 'return 1' calc.py; then\n  echo '{\"outcome\":\"pass\",\"message\":\"val \
         returns 1\"}'\nelse\n  echo '{\"outcome\":\"fail\",\"message\":\"val returned 0\"}'\nfi\n",
    )?;
    let binding = json!({
        "args": ["checks/val.sh"],
        "cache": true,
        "inputs": ["calc.py", "checks/val.sh"],
        "program": "sh",
        "protocol": "json",
        "timeout_seconds": 3_u8,
        "version": 1_u8
    });
    write_file(
        root,
        ".contracts/001-rule/checks/val.json",
        &binding.to_string(),
    )?;
    drop(run_git(root, &["init", "-q"])?);
    commit(root, "baseline")?;
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "CLI integration test assertions verify exit codes for rejected flags."
)]
fn check_rejects_local_and_requires_model_credentials() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path();
    create_failing_check_repo(root)?;

    for args in [
        &["check", "--local"][..],
        &["status", "--local"][..],
        &["check", "--no-model"][..],
        &["check"][..],
    ] {
        let output = run_tenet(root, args)?;
        assert_eq!(
            output.status.code(),
            Some(2_i32),
            "expected exit 2 for {args:?}, got stderr: {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
#[expect(
    clippy::indexing_slicing,
    clippy::panic_in_result_fn,
    reason = "CLI integration test assertions inspect known status output and JSON fields."
)]
fn status_surfaces_behavior_failure_without_contradicting_model_verdict() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path();
    create_failing_check_repo(root)?;

    let provider = Provider::start(&[], "uncertain")?;
    let checked = provider.run_tenet(root, &["check", "--final", "--fresh"])?;
    assert!(checked.status.code().is_some());

    let human = run_tenet(root, &["status"])?;
    assert_eq!(human.status.code(), Some(4_i32));
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(
        stdout.contains("behavior failed"),
        "expected behavior failed on contract row: {stdout:?}"
    );
    assert!(
        !stdout.contains("leans no violation"),
        "behavior failure should not show leans no violation: {stdout:?}"
    );

    let json_out = run_tenet(root, &["status", "--json"])?;
    assert_eq!(json_out.status.code(), Some(4_i32));
    let doc: Value = serde_json::from_slice(&json_out.stdout)?;
    assert_eq!(doc["contracts"][0]["behavior_failed"], json!(true));
    assert_eq!(doc["behavior_checks"][0]["outcome"], json!("failed"));

    // When the semantic model passes ("no_match") while an executable witness still fails,
    // status must exit 1 (Failed) rather than 0.
    let pass_provider = Provider::start(&[], "no_match")?;
    let _checked_pass = pass_provider.run_tenet(root, &["check", "--final", "--fresh"])?;
    let failed_status = run_tenet(root, &["status", "--json"])?;
    assert_eq!(failed_status.status.code(), Some(1_i32));
    let failed_doc: Value = serde_json::from_slice(&failed_status.stdout)?;
    assert_eq!(failed_doc["contracts"][0]["behavior_failed"], json!(true));
    assert_eq!(failed_doc["behavior_checks"][0]["outcome"], json!("failed"));
    Ok(())
}
