use std::fs;

use anyhow::Result;
use tenet_checks::behavior::{
    AuditContractOutput,
    AuditModuleInput,
    AuditModuleReport,
    Finding,
    Outcome,
    SeverityLevel,
};

use super::groups::build_module_group;

fn build_finding(
    check: &str,
    message: &str,
    outcome: Outcome,
    severity: Option<&str>,
    subject: &str,
) -> Finding {
    Finding {
        check: check.to_owned(),
        contract: String::from("demo"),
        cost_usd: None,
        explanation_cost_usd: None,
        explanation_requests: None,
        generation_requests: Some(1),
        message: message.to_owned(),
        outcome,
        pin_reused: Some(false),
        repro: None,
        requested: true,
        reused: false,
        severity: severity.map(str::to_owned),
        subject: Some(subject.into()),
    }
}

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
        findings: vec![build_finding(
            "demo-check",
            "Observed mismatch.",
            Outcome::Failed,
            None,
            subject,
        )],
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

fn create_core_rs_fixture() -> Result<(tempfile::TempDir, Vec<String>)> {
    let dir = tempfile::tempdir()?;
    fs::create_dir_all(dir.path().join("src"))?;
    fs::write(dir.path().join("src/core.rs"), "pub fn run() {}\n")?;
    Ok((dir, vec![String::from("src/core.rs")]))
}

#[test]
fn build_module_group_populates_test_files_from_exact_and_overlapping_modules() -> Result<()> {
    let (dir, runtime_paths) = create_core_rs_fixture()?;
    let root = dir.path();
    let discovered = vec![build_discovered_module(
        "src",
        &["src/core.rs"],
        &["tests/core_test.rs", "tests/integration_test.rs"],
    )];

    let exact_report = build_failing_report("src", "src/core.rs");
    let exact_group = build_module_group(
        root,
        &exact_report,
        None,
        &[],
        SeverityLevel::Medium,
        &discovered,
        &runtime_paths,
    )
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
        SeverityLevel::Medium,
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

fn build_mixed_severity_report() -> AuditModuleReport {
    AuditModuleReport {
        contracts: vec![AuditContractOutput {
            document: String::from("Keep valid.\n"),
            lock_path: String::from("audit-pins/demo/scripts/.lock.json"),
            message: String::from("Keep valid."),
            name: String::from("demo"),
            paths: vec![String::from("src/core.rs")],
        }],
        cost_usd: None,
        findings: vec![
            build_finding(
                "crit-check",
                "[critical] Corrupts state.",
                Outcome::Failed,
                Some("critical"),
                "src/core.rs",
            ),
            build_finding(
                "high-check",
                "[high] Drops header.",
                Outcome::Failed,
                None,
                "src/core.rs",
            ),
            build_finding(
                "med-check",
                "Minor edge mismatch.",
                Outcome::Failed,
                Some("medium"),
                "src/core.rs",
            ),
            build_finding(
                "guard-pass",
                "Verified invariant.",
                Outcome::Passed,
                None,
                "src/core.rs",
            ),
        ],
        name: String::from("src"),
        requests: 1,
    }
}

#[test]
fn build_module_group_filters_witnesses_by_severity_while_preserving_all_guards() -> Result<()> {
    let (dir, runtime_paths) = create_core_rs_fixture()?;
    let root = dir.path();
    let report = build_mixed_severity_report();
    let collect_ids = |level: SeverityLevel| -> Result<(Vec<String>, usize)> {
        let group = build_module_group(root, &report, None, &[], level, &[], &runtime_paths)
            .ok_or_else(|| anyhow::anyhow!("expected fix group for {level:?}"))?;
        let ids = group
            .witnesses
            .iter()
            .map(|item| item.check.clone())
            .collect();
        anyhow::ensure!(
            group
                .guards
                .first()
                .is_some_and(|guard| guard.check == "guard-pass")
        );
        Ok((ids, group.guards.len()))
    };
    let (high_ids, high_guards) = collect_ids(SeverityLevel::High)?;
    anyhow::ensure!(high_ids == vec!["crit-check", "high-check"] && high_guards == 1);
    let (crit_ids, crit_guards) = collect_ids(SeverityLevel::Critical)?;
    anyhow::ensure!(crit_ids == vec!["crit-check"] && crit_guards == 1);
    Ok(())
}
