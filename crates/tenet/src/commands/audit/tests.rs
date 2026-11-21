use std::fs;

use anyhow::Result;
use tenet_checks::behavior::{AuditContractOutput, AuditModuleReport, Finding, Outcome};

use super::{discover::discover_modules, queue::AuditQueue, save::save_verified_contracts};

fn build_sample_report() -> AuditModuleReport {
    AuditModuleReport {
        contracts: vec![AuditContractOutput {
            document: String::from(
                "---\nname: demo-rule\nmessage: Keep demo valid.\npaths:\n  - \
                 'src/app.py'\n---\n\n## Rules\n\nKeep demo valid.\n",
            ),
            lock_path: String::from("audit-pins/demo-rule/agent-checks.lock.json"),
            message: String::from("Keep demo valid."),
            name: String::from("demo-rule"),
            paths: vec![String::from("src/app.py")],
        }],
        cost_usd: Some(0.02_f64),
        findings: vec![Finding {
            check: String::from("demo-check"),
            contract: String::from("demo-rule"),
            cost_usd: Some(0.01_f64),
            explanation_cost_usd: None,
            explanation_requests: None,
            generation_requests: Some(1),
            message: String::from("Observed mismatch."),
            outcome: Outcome::Failed,
            pin_reused: Some(false),
            requested: true,
            reused: false,
            subject: Some("src/app.py".into()),
        }],
        name: String::from("src"),
        requests: 2,
    }
}

#[test]
fn discover_modules_skips_tests_and_attaches_manifests() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    fs::create_dir_all(root.join("src/pkg"))?;
    fs::create_dir_all(root.join("tests"))?;
    fs::create_dir_all(root.join("testdata"))?;
    fs::write(root.join("pyproject.toml"), "[project]\nname = \"demo\"\n")?;
    fs::write(root.join("README.md"), "# Demo\n")?;
    fs::write(root.join("src/pkg/core.py"), "def run():\n    return 1\n")?;
    fs::write(
        root.join("tests/test_core.py"),
        "def test_run():\n    assert True\n",
    )?;
    fs::write(root.join("testdata/fixture.py"), "X = 1\n")?;

    let discovered = discover_modules(root, &[])?;
    let count = discovered.modules.len();
    anyhow::ensure!(count == 1, "expected 1 module, got {count:?}");
    let first = discovered
        .modules
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing module"))?;
    let name = &first.name;
    anyhow::ensure!(name == "src/pkg", "unexpected name: {name:?}");
    anyhow::ensure!(first.files == vec!["src/pkg/core.py"]);
    anyhow::ensure!(first.manifests == vec!["pyproject.toml"]);
    anyhow::ensure!(first.docs == vec!["README.md"]);
    Ok(())
}

#[test]
fn queue_round_trip_and_save_verified_contracts() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let state = root.join(".tenet");
    fs::create_dir_all(state.join("audit-pins/demo-rule"))?;
    fs::write(
        state.join("audit-pins/demo-rule/agent-checks.lock.json"),
        "{\"protocol\":1}\n",
    )?;
    let report = build_sample_report();
    let mut queue = AuditQueue::new(true);
    queue.upsert(report.clone());
    queue.save(&state)?;

    let loaded = AuditQueue::read(&state, true, false)?;
    anyhow::ensure!(loaded.has_module("src"), "expected saved module in queue");

    let saved = save_verified_contracts(root, &state, &[report])?;
    anyhow::ensure!(saved.len() == 1, "expected 1 saved contract");
    let first = saved
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing saved contract"))?;
    anyhow::ensure!(first.pin_saved, "expected staged pin to be copied");
    anyhow::ensure!(root.join(".contracts/001-demo-rule/CONTRACT.md").is_file());
    anyhow::ensure!(
        root.join(".contracts/001-demo-rule/agent-checks.lock.json")
            .is_file()
    );
    Ok(())
}
