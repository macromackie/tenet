use std::{fs, path::Path};

use anyhow::Result;
use serde_json::{Value, json};
use tenet_checks::behavior::{AuditContractOutput, AuditModuleReport, Finding, Outcome};

use super::super::{
    replay::{StatusRow, write_status_row},
    save::save_verified_contracts,
};

fn build_sharded_report(
    module_name: &str,
    contract_name: &str,
    check_id: &str,
    subject: &str,
    lock_path: &str,
) -> AuditModuleReport {
    AuditModuleReport {
        contracts: vec![AuditContractOutput {
            document: format!(
                "---\nname: {contract_name}\nmessage: Keep {contract_name} valid.\npaths:\n  - \
                 '{subject}'\n---\n\n## Rules\n\nKeep {contract_name} valid.\n\n## Notes\n\nExtra \
                 details for {contract_name}.\n"
            ),
            lock_path: lock_path.to_owned(),
            message: format!("Keep {contract_name} valid."),
            name: contract_name.to_owned(),
            paths: vec![subject.to_owned()],
        }],
        cost_usd: None,
        findings: vec![Finding {
            check: check_id.to_owned(),
            contract: contract_name.to_owned(),
            cost_usd: None,
            explanation_cost_usd: None,
            explanation_requests: None,
            generation_requests: Some(1),
            message: format!("Verified {check_id}."),
            outcome: Outcome::Passed,
            pin_reused: Some(false),
            repro: None,
            requested: true,
            reused: false,
            severity: None,
            subject: Some(subject.into()),
        }],
        name: module_name.to_owned(),
        requests: 1,
    }
}

fn write_staged_lock(state: &Path, lock_rel: &str, check_id: &str, subject: &str) -> Result<()> {
    let lock_path = state.join(lock_rel);
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = json!({
        "protocol": 1_u64,
        "shape": format!("shape-{check_id}"),
        "bundle": {
            "checks": [{
                "id": check_id,
                "promise": format!("Preserve {check_id}."),
                "runtime": "python",
                "subject": subject,
                "test": format!("print('{check_id}')\n"),
                "good": "def ok(): return 1\n",
                "defect": "def ok(): return 0\n"
            }],
            "open": []
        }
    });
    fs::write(&lock_path, serde_json::to_string_pretty(&payload)?)?;
    Ok(())
}

fn assert_consolidated_bundle(root: &Path, expected_hash: &str) -> Result<()> {
    anyhow::ensure!(root.join(".contracts/001-src/scripts/check-a.py").is_file());
    anyhow::ensure!(root.join(".contracts/001-src/scripts/check-b.py").is_file());
    let lock_text = fs::read_to_string(root.join(".contracts/001-src/scripts/.lock.json"))?;
    let lock_json: Value = serde_json::from_str(&lock_text)?;
    anyhow::ensure!(
        lock_json.get("contractHash").and_then(Value::as_str) == Some(expected_hash),
        "contractHash mismatch: {lock_json:?}"
    );
    let checks = lock_json
        .pointer("/bundle/checks")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("missing merged bundle checks"))?;
    anyhow::ensure!(
        checks.len() == 2,
        "expected 2 merged checks, got {:?}",
        checks.len()
    );
    anyhow::ensure!(
        checks
            .iter()
            .all(|entry| entry.get("good").is_none() && entry.get("defect").is_none()),
        "expected slim lock without bulky control source"
    );
    Ok(())
}

fn build_two_shard_reports(state: &Path) -> Result<Vec<AuditModuleReport>> {
    write_staged_lock(
        state,
        "audit-pins/alpha/agent-checks.lock.json",
        "check-a",
        "src/a.py",
    )?;
    write_staged_lock(
        state,
        "audit-pins/beta/agent-checks.lock.json",
        "check-b",
        "src/b.py",
    )?;
    Ok(vec![
        build_sharded_report(
            "src#1",
            "alpha-rule",
            "check-a",
            "src/a.py",
            "audit-pins/alpha/agent-checks.lock.json",
        ),
        build_sharded_report(
            "src#2",
            "beta-rule",
            "check-b",
            "src/b.py",
            "audit-pins/beta/agent-checks.lock.json",
        ),
    ])
}

#[test]
fn save_verified_contracts_consolidates_sharded_module_contracts() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let state = root.join(".tenet");
    let reports = build_two_shard_reports(&state)?;

    let saved = save_verified_contracts(root, &state, &reports)?;
    anyhow::ensure!(
        saved.len() == 1,
        "expected 1 consolidated contract, got {saved:?}"
    );
    let only = saved
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing consolidated contract"))?;
    anyhow::ensure!(only.name == "src", "unexpected slug {:?}", only.name);
    anyhow::ensure!(only.pin_saved, "expected merged pin bundle to be written");

    let doc_rel = Path::new(".contracts/001-src/CONTRACT.md");
    let document = fs::read_to_string(root.join(doc_rel))?;
    let parsed = tenet_contracts::parse(doc_rel, &document)?;
    anyhow::ensure!(parsed.name == "src");
    anyhow::ensure!(parsed.message == "Behavioral guarantees for src.");
    anyhow::ensure!(document.contains("### alpha-rule") && document.contains("### beta-rule"));
    anyhow::ensure!(
        document.matches("---").count() == 2,
        "duplicate frontmatter in {document:?}"
    );
    assert_consolidated_bundle(root, &parsed.hash)
}

#[test]
fn write_status_row_omits_duplicate_expected_observed_line() -> Result<()> {
    let mut repeated = Vec::new();
    write_status_row(
        &mut repeated,
        &StatusRow {
            check: "keeps-zero",
            contract: "demo",
            expected: Some("0"),
            label: String::from("FAIL"),
            marker: String::from("x"),
            message: "Expected 0, observed 7.",
            observed: Some("7"),
            subject: Some("src/choose.mjs"),
        },
    )?;
    let repeated_text = String::from_utf8(repeated)?;
    anyhow::ensure!(
        !repeated_text.contains("expected 0 \u{b7} got 7"),
        "duplicate expected/got line in {repeated_text:?}"
    );

    let mut concise = Vec::new();
    write_status_row(
        &mut concise,
        &StatusRow {
            check: "keeps-zero",
            contract: "demo",
            expected: Some("0"),
            label: String::from("FAIL"),
            marker: String::from("x"),
            message: "Observed mismatch.",
            observed: Some("7"),
            subject: Some("src/choose.mjs"),
        },
    )?;
    let concise_text = String::from_utf8(concise)?;
    anyhow::ensure!(
        concise_text.contains("expected 0 \u{b7} got 7"),
        "missing expected/got line in {concise_text:?}"
    );
    Ok(())
}
