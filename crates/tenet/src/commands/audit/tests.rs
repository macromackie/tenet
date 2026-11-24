mod save_consolidation;

use core::slice;
use std::{fs, process::Command};

use anyhow::Result;
use tenet_checks::behavior::{
    AuditContractOutput,
    AuditModuleReport,
    Finding,
    Outcome,
    WitnessRepro,
};

use super::{
    discover::discover_modules,
    queue::AuditQueue,
    repros::write_witness_repros,
    save::save_verified_contracts,
};

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
            repro: Some(WitnessRepro {
                code: String::from("print(1)\n"),
                expected: String::from("2"),
                observed: Some(String::from("1")),
                runtime: String::from("python"),
            }),
            requested: true,
            reused: false,
            severity: None,
            subject: Some("src/app.py".into()),
        }],
        name: String::from("src"),
        requests: 2,
    }
}

#[test]
fn discover_modules_keeps_generated_runtime_files_and_merges_tiny_siblings() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    fs::create_dir_all(root.join("src/gen"))?;
    fs::create_dir_all(root.join("src/locale"))?;
    fs::write(root.join("src/index.js"), "export default () => 1;\n")?;
    fs::write(root.join("src/gen/visit.rs"), "pub fn visit() {}\n")?;
    fs::write(root.join("src/locale/fr.js"), "export default 'fr';\n")?;
    for plugin in ["alpha", "beta", "gamma", "delta", "epsilon"] {
        fs::create_dir_all(root.join(format!("src/plugin/{plugin}")))?;
        fs::write(
            root.join(format!("src/plugin/{plugin}/index.js")),
            format!("export const name = '{plugin}';\n"),
        )?;
    }

    let discovered = discover_modules(root, &[], None)?;
    let module_names: Vec<&str> = discovered
        .modules
        .iter()
        .map(|module| module.name.as_str())
        .collect();
    anyhow::ensure!(
        module_names == vec!["src", "src/plugin"],
        "unexpected modules: {module_names:?}"
    );
    anyhow::ensure!(
        discovered
            .runtime_paths
            .contains(&String::from("src/gen/visit.rs")),
        "expected generated file in runtime_paths"
    );
    anyhow::ensure!(
        discovered
            .runtime_paths
            .contains(&String::from("src/locale/fr.js")),
        "expected locale file in runtime_paths"
    );
    Ok(())
}

#[test]
fn discover_modules_ranks_docs_and_flags_low_test_files() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    fs::create_dir_all(root.join("src/parser"))?;
    fs::create_dir_all(root.join("src/format"))?;
    fs::create_dir_all(root.join("tests"))?;
    fs::create_dir_all(root.join("docs"))?;
    fs::write(root.join("README.md"), "# Demo\n")?;
    for name in ["alpha", "beta", "gamma", "delta", "parser"] {
        fs::write(root.join(format!("docs/{name}.rst")), format!("{name}\n"))?;
    }
    fs::write(
        root.join("src/parser/lexer.py"),
        "from ..format import render\ndef lex():\n    return render()\n",
    )?;
    fs::write(
        root.join("src/format/render.py"),
        "def render():\n    return 1\n",
    )?;
    fs::write(root.join("tests/test_render_one.py"), "import render\n")?;
    fs::write(root.join("tests/test_render_two.py"), "import render\n")?;

    let discovered = discover_modules(root, &[], None)?;
    let first = discovered
        .modules
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing first module"))?;
    let name = &first.name;
    anyhow::ensure!(
        name == "src/parser",
        "expected low-test module first, got {name:?}"
    );
    anyhow::ensure!(first.low_test_files == vec!["src/parser/lexer.py"]);
    anyhow::ensure!(
        first.related_files == vec!["src/format/render.py"],
        "expected imported seam file in {:?}",
        first.related_files
    );
    anyhow::ensure!(
        first.docs.contains(&String::from("docs/parser.rst")),
        "expected relevance-matched doc in {:?}",
        first.docs
    );
    Ok(())
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

    let discovered = discover_modules(root, &[], None)?;
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
    anyhow::ensure!(
        first.test_files == vec!["tests/test_core.py"],
        "expected matched test file, got {:?}",
        first.test_files
    );
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
        "{\"protocol\":1,\"bundle\":{\"checks\":[{\"id\":\"demo-check\",\"promise\":\"Keep demo \
         valid.\",\"runtime\":\"python\",\"subject\":\"src/app.py\",\"test\":\"print(1)\\n\"}]}}\n",
    )?;
    let report = build_sample_report();
    let mut queue = AuditQueue::new(true);
    queue.upsert(report.clone());
    queue.save(&state)?;

    let loaded = AuditQueue::read(&state, true, false)?;
    anyhow::ensure!(loaded.has_module("src"), "expected saved module in queue");
    let loaded_repro = loaded
        .modules
        .get("src")
        .and_then(|item| item.findings.first())
        .and_then(|finding| finding.repro.as_ref());
    anyhow::ensure!(
        loaded_repro.is_some_and(|repro| repro.runtime == "python" && repro.expected == "2"),
        "expected witness reproducer to round-trip through audit queue"
    );

    let saved = save_verified_contracts(root, &state, slice::from_ref(&report))?;
    anyhow::ensure!(saved.len() == 1, "expected 1 saved contract");
    let first = saved
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing saved contract"))?;
    anyhow::ensure!(first.pin_saved, "expected staged pin to be copied");
    anyhow::ensure!(root.join(".contracts/001-demo-rule/CONTRACT.md").is_file());
    anyhow::ensure!(
        root.join(".contracts/001-demo-rule/scripts/.lock.json")
            .is_file()
    );
    anyhow::ensure!(
        root.join(".contracts/001-demo-rule/scripts/demo-check.py")
            .is_file()
    );
    let written = write_witness_repros(&root.join("repros"), &[report])?;
    let repro_file = written
        .get(&(String::from("demo-rule"), String::from("demo-check")))
        .ok_or_else(|| anyhow::anyhow!("missing written repro path"))?;
    anyhow::ensure!(
        repro_file.extension().and_then(|ext| ext.to_str()) == Some("py")
            && fs::read_to_string(repro_file)? == "print(1)\n",
        "unexpected repro file {:?}",
        repro_file.display()
    );
    Ok(())
}

#[test]
fn discover_modules_filters_to_changed_modules_for_base_ref() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    fs::create_dir_all(root.join("src/alpha"))?;
    fs::create_dir_all(root.join("src/beta"))?;
    fs::write(root.join("src/alpha/a.py"), "def a():\n    return 1\n")?;
    fs::write(root.join("src/beta/b.py"), "def b():\n    return 2\n")?;
    for args in [
        &["init", "-q"][..],
        &["config", "user.email", "t@example.com"][..],
        &["config", "user.name", "Tenet"][..],
        &["add", "-A"][..],
        &["commit", "-q", "-m", "base"][..],
    ] {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()?;
        anyhow::ensure!(status.success(), "git {args:?} failed");
    }
    fs::write(root.join("src/beta/b.py"), "def b():\n    return 3\n")?;

    let discovered = discover_modules(root, &[], Some("HEAD"))?;
    let module_count = discovered.modules.len();
    anyhow::ensure!(
        module_count == 1,
        "expected only changed module, got {module_count:?}",
    );
    let only = discovered
        .modules
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing changed module"))?;
    anyhow::ensure!(only.name == "src/beta");
    anyhow::ensure!(only.changed_files == vec!["src/beta/b.py"]);
    anyhow::ensure!(
        discovered
            .runtime_paths
            .contains(&String::from("src/alpha/a.py")),
        "expected full runtime closure to retain unchanged sibling"
    );
    Ok(())
}
