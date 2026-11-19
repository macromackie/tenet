//! Catalog suggestions come from a release and cite local evidence without writing the project.

#[path = "support/build_tenet.rs"]
mod build_tenet;
#[path = "support/commit.rs"]
mod commit;
#[path = "support/run_git.rs"]
mod run_git;
#[path = "support/write_file.rs"]
mod write_file;

use std::path::Path;

use anyhow::{Context as _, Result, ensure};
use serde_json::Value;
use tempfile::tempdir;

use self::{build_tenet::build_tenet, commit::commit, run_git::run_git, write_file::write_file};

const INPUTS: &str = "---\nname: code-explicit-input-precedence\nmessage: Use an explicit input \
                      before its fallback.\n---\n\n## Rules\n\nUse the explicit input.";

fn read_suggestions(root: &Path, library: &Path) -> Result<Value> {
    let result = build_tenet()
        .arg("--root")
        .arg(root)
        .args(["contracts", "recommend", "--json", "--library"])
        .arg(library)
        .env_remove("OPENROUTER_API_KEY")
        .env_remove("TYPESAFE_API_KEY")
        .env_remove("TENET_PROVIDER")
        .output()?;
    ensure!(
        result.status.success(),
        "{:?}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(serde_json::from_slice(&result.stdout)?)
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the release, evidence and absence of writes."
)]
fn recommends_the_tagged_contract_for_a_public_export_before_adoption() -> Result<()> {
    let project = tempdir()?;
    let catalog = tempdir()?;
    let root = project.path();
    let library = catalog.path();
    release_catalog(library)?;
    write_file(
        root,
        "src/client.ts",
        "export function createClient(timeout?: number) { return timeout ?? 30; }",
    )?;
    write_file(
        root,
        "scripts/release.py",
        "def get_timeout():\n    return 5\n",
    )?;
    let first = read_suggestions(root, library)?;
    assert_eq!(first.get("release").and_then(Value::as_str), Some("v1.0.0"));
    let rows = first
        .get("recommendations")
        .and_then(Value::as_array)
        .context("recommendations")?;
    assert_eq!(rows.len(), 1);
    let candidate = rows.first().context("input contract")?;
    assert_eq!(
        candidate.get("path").and_then(Value::as_str),
        Some("src/client.ts")
    );
    assert!(
        candidate
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason.contains("createClient"))
    );
    assert!(!root.join(".contracts").exists() && !root.join(".tenet.lock").exists());
    write_file(root, ".contracts/001-local/CONTRACT.md", INPUTS)?;
    let adopted = read_suggestions(root, library)?;
    assert!(
        adopted
            .get("recommendations")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
    );
    Ok(())
}

fn release_catalog(library: &Path) -> Result<()> {
    drop(run_git(library, &["init", "-q"])?);
    write_file(
        library,
        "contracts/code-explicit-input-precedence/CONTRACT.md",
        INPUTS,
    )?;
    write_file(
        library,
        "contracts/code-python-options/CONTRACT.md",
        "---\nname: code-python-options\nmessage: Check Python fallback inputs.\n---\n\n## \
         Rules\n\nCheck Python.",
    )?;
    commit(library, "public catalog")?;
    drop(run_git(library, &["tag", "v1.0.0"])?);
    write_file(
        library,
        "contracts/code-explicit-input-precedence/CONTRACT.md",
        "invalid unpublished edit",
    )?;
    Ok(())
}
