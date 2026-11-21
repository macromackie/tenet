//! Verify that independent checker evidence survives semantic and reviewer approvals.

use std::path::Path;

use anyhow::{Context as _, Result, ensure};
use serde_json::json;

use super::{Provider, create_project, read_document, run_tenet, write_file};

fn approve_values(root: &Path) -> Result<()> {
    let record = run_tenet(
        root,
        &[
            "record",
            "values",
            "--conclusion",
            "verified",
            "--evidence",
            "src/value.txt:1",
            "-m",
            "Reviewer approved the current source.",
        ],
    )?;
    ensure!(
        record.status.success(),
        "{:?}",
        String::from_utf8_lossy(&record.stderr)
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Check independent review and executable outcomes."
)]
fn a_recorded_approval_cannot_hide_a_current_executable_failure() -> Result<()> {
    let fixture = tempfile::tempdir()?;
    let root = fixture.path();
    create_project(
        root,
        r#"if [ "$(cat src/value.txt)" = 0 ]; then
  echo '{"outcome":"pass","message":"zero preserved"}'
else
  echo '{"outcome":"fail","message":"expected zero, read one"}'
fi"#,
    )?;
    write_file(root, "src/value.txt", "1")?;
    let provider = Provider::start(&[], "no_match")?;
    ensure!(provider.run_tenet(root, &["check"])?.status.success());
    approve_values(root)?;
    let final_run = provider.run_tenet(
        root,
        &["check", "--final", "--contract", "values", "--json"],
    )?;
    ensure!(final_run.status.code() == Some(1_i32));
    let result = provider.run_tenet(root, &["check"])?;
    assert_eq!(
        result.status.code(),
        Some(1_i32),
        "{:?}",
        String::from_utf8_lossy(&result.stderr)
    );
    for (args, code, pointer) in [
        (vec!["status", "--json"], 1_i32, "/behavior_checks/0"),
        (
            vec!["show", "--pending", "--json"],
            0_i32,
            "/contracts/0/behavior_checks/0",
        ),
    ] {
        let document = read_document(&run_tenet(root, &args)?, code)?;
        let row = document
            .pointer(pointer)
            .context("missing executable finding")?;
        assert_eq!(row.get("outcome"), Some(&json!("failed")));
        assert_eq!(row.get("message"), Some(&json!("expected zero, read one")));
    }
    Ok(())
}
