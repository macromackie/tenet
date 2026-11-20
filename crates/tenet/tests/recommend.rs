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
use tempfile::{TempDir, tempdir};

use self::{build_tenet::build_tenet, commit::commit, run_git::run_git, write_file::write_file};

const INPUTS: &str = "---\nname: code-explicit-input-precedence\nmessage: Use an explicit input \
                      before its fallback.\n---\n\n## Rules\n\nUse the explicit input.";

fn create_fixture() -> Result<(TempDir, TempDir)> {
    let project = tempdir()?;
    let catalog = tempdir()?;
    release_catalog(catalog.path())?;
    Ok((project, catalog))
}

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
    let (project, catalog) = create_fixture()?;
    let root = project.path();
    let library = catalog.path();
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

fn read_suggestion_names(root: &Path, library: &Path) -> Result<Vec<String>> {
    let result = read_suggestions(root, library)?;
    let rows = result
        .get("recommendations")
        .and_then(Value::as_array)
        .context("recommendations")?;
    Ok(rows
        .iter()
        .filter_map(|item| item.get("contract").and_then(Value::as_str))
        .map(str::to_owned)
        .collect())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert precision against domain-irrelevant catalog entries."
)]
fn avoids_false_positive_service_and_instruction_matches_for_a_python_library() -> Result<()> {
    let (project, catalog) = create_fixture()?;
    let root = project.path();
    let library = catalog.path();
    write_file(
        root,
        "src/cli/types.py",
        concat!(
            "class File:\n",
            "    def collect(self, items, default=None):\n",
            "        if default is None:\n",
            "            items.append('x')\n",
            "        return items\n",
        ),
    )?;
    let names = read_suggestion_names(root, library)?;
    assert!(
        names
            .iter()
            .any(|name| name == "code-explicit-input-precedence"),
        "{names:?}"
    );
    assert!(
        names.iter().any(|name| name == "code-borrowed-inputs"),
        "{names:?}"
    );
    assert!(
        !names
            .iter()
            .any(|name| name == "context-agent-instruction-files"),
        "{names:?}"
    );
    assert!(
        !names
            .iter()
            .any(|name| name == "service-mutation-idempotency-keys"),
        "{names:?}"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert FastAPI term matching and TypeScript non-script precision."
)]
fn matches_fastapi_routes_and_avoids_script_false_positives_on_typescript_libraries() -> Result<()>
{
    let (project, catalog) = create_fixture()?;
    let root = project.path();
    let library = catalog.path();
    write_file(
        root,
        "src/routes.py",
        "from fastapi import APIRouter\n\ndef cancel_order(order_id: str):\n    return order_id\n",
    )?;
    write_file(
        root,
        "src/sdk.ts",
        "export function resolveTimeout(timeout?: number) { return timeout ?? 5; }\n",
    )?;
    let names = read_suggestion_names(root, library)?;
    assert!(
        names
            .iter()
            .any(|name| name == "api-fastapi-route-authorization"),
        "{names:?}"
    );
    assert!(
        !names
            .iter()
            .any(|name| name == "code-typescript-node-scripts"),
        "{names:?}"
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
        "contracts/code-borrowed-inputs/CONTRACT.md",
        "---\nname: code-borrowed-inputs\nmessage: Preserve caller-owned values.\n---\n\n## \
         Rules\n\nPreserve borrowed inputs.",
    )?;
    write_file(
        library,
        "contracts/context-agent-instruction-files/CONTRACT.md",
        "---\nname: context-agent-instruction-files\nmessage: Keep agent instruction files \
         accurate.\n---\n\n## Rules\n\nKeep agent instruction files accurate.",
    )?;
    write_file(
        library,
        "contracts/service-mutation-idempotency-keys/CONTRACT.md",
        "---\nname: service-mutation-idempotency-keys\nmessage: Guard service mutations with \
         idempotency keys.\n---\n\n## Rules\n\nGuard service mutations.",
    )?;
    write_file(
        library,
        "contracts/code-python-options/CONTRACT.md",
        "---\nname: code-python-options\nmessage: Check Python fallback inputs.\n---\n\n## \
         Rules\n\nCheck Python.",
    )?;
    write_file(
        library,
        "contracts/api-fastapi-route-authorization/CONTRACT.md",
        "---\nname: api-fastapi-route-authorization\nmessage: Authorize every FastAPI \
         route.\n---\n\n## Rules\n\nAuthorize routes.",
    )?;
    write_file(
        library,
        "contracts/code-typescript-node-scripts/CONTRACT.md",
        "---\nname: code-typescript-node-scripts\nmessage: Keep TypeScript Node scripts \
         portable.\n---\n\n## Rules\n\nKeep scripts portable.",
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
