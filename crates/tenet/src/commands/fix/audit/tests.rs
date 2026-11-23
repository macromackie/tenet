use std::fs;

use anyhow::Result;
use tenet_checks::behavior::{
    AuditContractOutput,
    AuditModuleInput,
    AuditModuleReport,
    Finding,
    Outcome,
};

use super::groups::build_module_group;

fn build_failing_report(module_name: &str, subject: &str) -> AuditModuleReport {
    AuditModuleReport {
        contracts: vec![AuditContractOutput {
            document: String::from("Keep valid.\n"),
            lock_path: String::from("audit-pins/demo/agent-checks.lock.json"),
            message: String::from("Keep valid."),
            name: String::from("demo"),
            paths: vec![subject.to_owned()],
        }],
        cost_usd: None,
        findings: vec![Finding {
            check: String::from("demo-check"),
            contract: String::from("demo"),
            cost_usd: None,
            explanation_cost_usd: None,
            explanation_requests: None,
            generation_requests: Some(1),
            message: String::from("Observed mismatch."),
            outcome: Outcome::Failed,
            pin_reused: Some(false),
            repro: None,
            requested: true,
            reused: false,
            subject: Some(subject.into()),
        }],
        name: module_name.to_owned(),
        requests: 1,
    }
}

fn build_discovered_module(name: &str, files: &[&str], test_files: &[&str]) -> AuditModuleInput {
    AuditModuleInput {
        changed_files: Vec::new(),
        docs: Vec::new(),
        files: files.iter().map(|path| (*path).to_owned()).collect(),
        low_test_files: Vec::new(),
        manifests: Vec::new(),
        name: name.to_owned(),
        related_files: Vec::new(),
        test_files: test_files.iter().map(|path| (*path).to_owned()).collect(),
        windows: Vec::new(),
    }
}

#[test]
fn build_module_group_populates_test_files_from_exact_and_overlapping_modules() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/core.rs"), "pub fn run() {}\n")?;
    let runtime_paths = vec![String::from("src/core.rs")];
    let discovered = vec![build_discovered_module(
        "src",
        &["src/core.rs"],
        &["tests/core_test.rs", "tests/integration_test.rs"],
    )];

    let exact_report = build_failing_report("src", "src/core.rs");
    let exact_group =
        build_module_group(root, &exact_report, None, &[], &discovered, &runtime_paths)
            .ok_or_else(|| anyhow::anyhow!("expected exact fix group"))?;
    anyhow::ensure!(
        exact_group.test_files == vec!["tests/core_test.rs", "tests/integration_test.rs"],
        "expected exact match test_files, got {:?}",
        exact_group.test_files
    );

    let resharded_report = build_failing_report("src#2", "src/core.rs");
    let overlap_group = build_module_group(
        root,
        &resharded_report,
        None,
        &[],
        &discovered,
        &runtime_paths,
    )
    .ok_or_else(|| anyhow::anyhow!("expected overlapping fix group"))?;
    anyhow::ensure!(
        overlap_group.test_files == vec!["tests/core_test.rs", "tests/integration_test.rs"],
        "expected overlapping fallback test_files, got {:?}",
        overlap_group.test_files
    );
    Ok(())
}
