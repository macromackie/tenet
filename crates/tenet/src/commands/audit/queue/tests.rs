use std::fs;

use anyhow::Result;
use tenet_checks::behavior::AuditModuleReport;

use super::AuditQueue;

#[test]
fn read_existing_errors_when_audit_state_is_missing() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let state = dir.path().join(".tenet");
    let result = AuditQueue::read_existing(&state);
    anyhow::ensure!(
        result.is_err(),
        "expected read_existing to fail when audit.json does not exist"
    );
    anyhow::ensure!(
        !state.join("audit.json").exists(),
        "read_existing must never create audit.json"
    );
    Ok(())
}

#[test]
fn read_existing_loads_saved_queue_without_mutating_file() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let state = dir.path().join(".tenet");
    let mut queue = AuditQueue::new(true);
    queue.upsert(AuditModuleReport {
        contracts: Vec::new(),
        cost_usd: Some(0.05_f64),
        findings: Vec::new(),
        name: String::from("src"),
        requests: 3,
    });
    queue.save(&state)?;
    let before = fs::read_to_string(state.join("audit.json"))?;

    let loaded = AuditQueue::read_existing(&state)?;
    anyhow::ensure!(loaded.deep, "expected deep flag to round-trip");
    anyhow::ensure!(loaded.has_module("src"), "expected saved module");
    anyhow::ensure!(
        fs::read_to_string(state.join("audit.json"))? == before,
        "read_existing must never overwrite audit.json"
    );
    Ok(())
}
