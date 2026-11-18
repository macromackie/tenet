#[cfg(unix)]
use std::path::Path;

use anyhow::Result;
use serde_json::{Value, json};

use super::fixture::run_tenet;
use crate::write_file::write_file;

#[test]
#[expect(
    clippy::indexing_slicing,
    clippy::panic_in_result_fn,
    reason = "A test reports its failure by panicking."
)]
fn status_lists_missing_obligations_before_the_first_review() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path();
    write_file(
        root,
        ".contracts/001-parsing/CONTRACT.md",
        "---\nname: parsing\nmessage: Keep present values.\nobligations: [keep-zero]\n---\n\nKeep \
         zero.",
    )?;
    write_file(root, "options.py", "VALUE = 0\n")?;
    write_file(root, "receipt.json", r#"{"version":1,"receipts":[]}"#)?;
    let receipt = root.join("receipt.json");
    let output = run_tenet(
        root,
        &[
            "status",
            "--json",
            "--evidence",
            receipt.to_str().unwrap_or_default(),
        ],
    )?;
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(output.status.code(), Some(4_i32));
    assert_eq!(document["revision"], Value::Null);
    assert_eq!(
        document["mechanical_evidence"][0]["obligation"],
        json!("keep-zero")
    );
    assert_eq!(
        document["mechanical_evidence"][0]["freshness"],
        json!("missing")
    );
    assert!(!root.join(".tenet").exists());
    Ok(())
}

#[cfg(unix)]
fn bind(root: &Path, name: &str, script: &str) -> Result<()> {
    write_file(root, &format!("checks/{name}.sh"), script)?;
    let binding = json!({
        "args": [format!("checks/{name}.sh")],
        "cache": true,
        "inputs": ["src/*.txt", format!("checks/{name}.sh")],
        "program": "sh",
        "protocol": "json",
        "timeout_seconds": 3_u8,
        "version": 1_u8
    });
    write_file(
        root,
        &format!(".contracts/001-values/checks/{name}.json"),
        &binding.to_string(),
    )
}

#[cfg(unix)]
#[test]
#[expect(
    clippy::indexing_slicing,
    clippy::panic_in_result_fn,
    reason = "Fixture assertions inspect known JSON fields."
)]
fn replays_a_counterexample_and_reuses_only_current_observations() -> Result<()> {
    let fixture = tempfile::tempdir()?;
    let root = fixture.path();
    write_file(
        root,
        ".contracts/001-values/CONTRACT.md",
        "---\nname: values\nmessage: Preserve zero.\npaths: ['src/*.txt']\nobligations: \
         [zero]\n---\nPreserve zero.",
    )?;
    write_file(root, "src/value.txt", "1")?;
    bind(
        root,
        "zero",
        r#"read -r value < src/value.txt
if [ "$value" = 0 ]; then
  echo '{"outcome":"pass","message":"zero retained"}'
else
  echo '{"outcome":"fail","message":"wrong value","finding":
    {"case":"zero","expected":"0","actual":"1","location":"src/value.txt"}}'
fi"#,
    )?;
    let check = |args: &[&str], code: i32| -> Result<Value> {
        let output = run_tenet(root, args)?;
        assert_eq!(
            output.status.code(),
            Some(code),
            "{:?}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout)?;
        Ok(result
            .get("checks")
            .unwrap_or_else(|| &result["mechanical_evidence"])[0]
            .clone())
    };
    let run = ["check", "--local", "values/zero", "--json"];
    let status = ["status", "--local", "--json"];
    assert_eq!(check(&status, 4_i32)?["freshness"], "missing");
    assert!(!root.join(".tenet").exists());
    let failed = check(&run, 1_i32)?;
    assert_eq!(failed["check"], "zero");
    assert_eq!(failed["finding"]["case"], "zero");
    assert_eq!(failed["replay"], "tenet check --local values/zero --fresh");
    assert_eq!(check(&run, 1_i32)?["reused"], true);
    write_file(root, "src/value.txt", "0")?;
    assert_eq!(check(&["status", "--json"], 4_i32)?["freshness"], "stale");
    assert_eq!(check(&run, 0_i32)?["reused"], false);
    write_file(root, "unrelated.md", "no change to the check")?;
    assert_eq!(check(&run, 0_i32)?["reused"], true);
    write_file(root, "src/new.txt", "new subject")?;
    assert_eq!(
        check(&status, 4_i32)?["reason"],
        "discovered input set changed"
    );
    assert_eq!(check(&run, 0_i32)?["reused"], false);
    assert_eq!(
        check(&["check", "--local", "--fresh", "--json"], 0_i32)?["reused"],
        false
    );
    Ok(())
}

#[cfg(unix)]
#[test]
#[expect(
    clippy::indexing_slicing,
    clippy::panic_in_result_fn,
    reason = "Fixture assertions inspect known JSON fields."
)]
fn incomplete_checkers_leave_their_neighbors_runnable() -> Result<()> {
    let fixture = tempfile::tempdir()?;
    let root = fixture.path();
    write_file(
        root,
        ".contracts/001-values/CONTRACT.md",
        "---\nname: values\nmessage: Preserve values.\nchecks: [first, missing, changes, \
         malformed, clean]\n---\nPreserve values.",
    )?;
    write_file(root, "src/value.txt", "1")?;
    bind(
        root,
        "first",
        r#"echo '{"outcome":"pass","message":"read before edit"}'"#,
    )?;
    bind(
        root,
        "changes",
        r#"echo changed > src/value.txt
echo '{"outcome":"pass","message":"asserted before edit"}'"#,
    )?;
    bind(root, "malformed", "echo invalid")?;
    bind(
        root,
        "clean",
        r#"echo '{"outcome":"pass","message":"completed"}'"#,
    )?;
    let output = run_tenet(root, &["check", "--local", "--json"])?;
    let report: Value = serde_json::from_slice(&output.stdout)?;
    let rows = &report["checks"];
    assert_eq!(output.status.code(), Some(4_i32));
    assert_eq!(rows[0]["freshness"], "stale");
    assert_eq!(rows[0]["observed_outcome"], "pass");
    assert_eq!(rows[1]["observed_outcome"], "unresolved");
    assert_eq!(rows[2]["freshness"], "unresolved");
    assert_eq!(rows[3]["observed_outcome"], "unresolved");
    assert_eq!(rows[4]["observed_outcome"], "pass");
    Ok(())
}
