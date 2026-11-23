use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

use serde_json::json;

use super::{
    AuditFixGroup,
    AuditFixRequest,
    AuditFixStatus,
    AuditFixWitness,
    AuditReplayRequest,
    AuditReplayTarget,
    Outcome,
    WitnessRepro,
    environment,
    process,
    replay_audit,
    request,
    run_audit_fix,
};

#[test]
fn embedded_worker_preserves_a_context_limit_without_provider_access() -> anyhow::Result<()> {
    let checkout = tempfile::tempdir()?;
    let root = checkout.path();
    let path = ".contracts/001-options/CONTRACT.md";
    fs::create_dir_all(root.join(".contracts/001-options"))?;
    fs::write(root.join(path), "Preserve explicit options.")?;
    fs::write(root.join("large.ts"), vec![b'x'; 140_000])?;
    let request = json!({
        "root": root,
        "state": root.join(".tenet"),
        "contracts": [{"name":"options", "hash":"frozen", "document":"Preserve explicit options.",
                       "path":path, "paths":["large.ts"]}],
        "maxGenerations": 0_usize,
    });
    let mut phases = Vec::new();
    let mut capture = |phase: &str| phases.push(phase.to_owned());
    let rows = process::execute(&request, root, false, Some(&mut capture))?;
    anyhow::ensure!(
        rows.iter()
            .any(|row| row.check == "coverage" && row.outcome == Outcome::Unresolved)
    );
    anyhow::ensure!(!phases.is_empty(), "expected live progress callback");
    Ok(())
}

#[test]
fn selected_source_keeps_its_contract_samples_and_respects_discovery() -> anyhow::Result<()> {
    let checkout = tempfile::tempdir()?;
    let root = checkout.path();
    let folder = root.join(".contracts/001-options");
    fs::create_dir_all(folder.join("assets/tenet"))?;
    fs::create_dir_all(folder.join("resources"))?;
    fs::write(
        folder.join("CONTRACT.md"),
        "---\nname: options\nmessage: Preserve explicit values.\npaths: [options.ts]\n---\nUse \
         the supplied value.\n",
    )?;
    fs::write(folder.join("assets/tenet/example.md"), "A public example.")?;
    fs::write(folder.join("assets/tenet/.env"), "a protected file")?;
    fs::write(folder.join("resources/usage.txt"), "A public reference.")?;
    fs::write(root.join(".ignore"), "private.txt\n")?;
    fs::write(folder.join("resources/private.txt"), "an ignored sample")?;
    fs::write(root.join("options.ts"), "export const value = 0;")?;
    let selection = tenet_engine::Selection {
        paths: vec!["options.ts".into()],
        ..Default::default()
    };
    let plan = tenet_engine::plan(root, &selection)?;
    let contract = plan
        .contracts
        .first()
        .ok_or_else(|| io::Error::other("missing contract"))?;
    let input = request::build_contract_input(&plan, contract, None, None, &[]);
    anyhow::ensure!(
        input.get("paths") == Some(&json!(["options.ts"])),
        "unexpected governed paths: {input:?}"
    );
    anyhow::ensure!(
        input.get("assets")
            == Some(&json!([
                ".contracts/001-options/assets/tenet/example.md",
                ".contracts/001-options/resources/usage.txt"
            ])),
        "unexpected reference paths: {input:?}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn resolve_node_binary_skips_version_manager_shims_for_real_node() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir()?;
    let shim_dir = dir.path().join("shims");
    let real_dir = dir.path().join("bin");
    fs::create_dir_all(&shim_dir)?;
    fs::create_dir_all(&real_dir)?;

    let mise = dir.path().join("mise");
    let real_node = real_dir.join("node");
    fs::write(&mise, "#!/bin/sh\n")?;
    fs::write(&real_node, "#!/bin/sh\n")?;
    symlink(&mise, shim_dir.join("node"))?;

    let resolved = environment::resolve_node_binary(&[shim_dir, real_dir]);
    let expected = real_node.canonicalize()?;
    anyhow::ensure!(
        resolved == expected,
        "expected {expected:?}, got {resolved:?}"
    );
    Ok(())
}

fn build_cargo_toml_repro() -> WitnessRepro {
    WitnessRepro {
        code: "import { readFileSync } from 'node:fs';\nconst text = readFileSync('Cargo.toml', \
               'utf8');\nconsole.log(JSON.stringify({ ok: text.includes('name = \
               \"tenet-checks\"') }));\n"
            .into(),
        expected: "{\"ok\":true}".into(),
        observed: Some("{\"ok\":false}".into()),
        runtime: "node".into(),
    }
}

#[test]
fn replay_audit_resolves_relative_root_against_caller_cwd() -> anyhow::Result<()> {
    let state_dir = tempfile::tempdir()?;
    let request = AuditReplayRequest {
        root: PathBuf::from("."),
        runtime_paths: vec!["Cargo.toml".into()],
        state: state_dir.path().to_path_buf(),
        targets: vec![AuditReplayTarget {
            check: "relative-root".into(),
            contract: "relative-root".into(),
            lock_path: None,
            repro: Some(build_cargo_toml_repro()),
            subject: Some("Cargo.toml".into()),
        }],
    };
    let response = replay_audit(&request, None)?;
    let result = response
        .results
        .first()
        .ok_or_else(|| io::Error::other("missing replay result"))?;
    anyhow::ensure!(
        result.outcome == Outcome::Passed,
        "expected relative root '.' to read {:?}/Cargo.toml, got {result:?}",
        Path::new(".").canonicalize()?
    );
    Ok(())
}

#[test]
fn run_audit_fix_decodes_host_payload_and_prechecks_passing_witness() -> anyhow::Result<()> {
    let state_dir = tempfile::tempdir()?;
    let request = AuditFixRequest {
        deep: false,
        groups: vec![AuditFixGroup {
            contracts: Vec::new(),
            files: vec!["Cargo.toml".into()],
            guards: Vec::new(),
            name: "root".into(),
            related_files: Vec::new(),
            test_files: Vec::new(),
            witnesses: vec![AuditFixWitness {
                check: "already-green".into(),
                contract: "already-green".into(),
                lock_path: None,
                message: "already passing".into(),
                repro: Some(build_cargo_toml_repro()),
                subject: Some("Cargo.toml".into()),
            }],
        }],
        max_requests: 0,
        root: PathBuf::from("."),
        runtime_paths: vec!["Cargo.toml".into()],
        state: state_dir.path().to_path_buf(),
    };
    let response = run_audit_fix(&request, None)?;
    let outcome = response
        .outcomes
        .first()
        .ok_or_else(|| io::Error::other("missing audit-fix outcome"))?;
    anyhow::ensure!(
        outcome.status == AuditFixStatus::AlreadyPassing && response.requests == 0,
        "expected already_passing with 0 requests, got {response:?}"
    );
    Ok(())
}

#[test]
fn audit_fix_group_serializes_test_files_and_omits_when_empty() -> anyhow::Result<()> {
    let empty_group = AuditFixGroup {
        contracts: Vec::new(),
        files: vec!["src/lib.rs".into()],
        guards: Vec::new(),
        name: "src".into(),
        related_files: Vec::new(),
        test_files: Vec::new(),
        witnesses: Vec::new(),
    };
    let empty_value = serde_json::to_value(&empty_group)?;
    anyhow::ensure!(
        empty_value.get("testFiles").is_none(),
        "expected empty test_files to be omitted, got {empty_value:?}"
    );
    let roundtrip_empty: AuditFixGroup = serde_json::from_value(empty_value)?;
    anyhow::ensure!(roundtrip_empty.test_files.is_empty());

    let populated_group = AuditFixGroup {
        test_files: vec!["tests/lib_test.rs".into()],
        ..empty_group
    };
    let populated_value = serde_json::to_value(&populated_group)?;
    anyhow::ensure!(
        populated_value.get("testFiles") == Some(&json!(["tests/lib_test.rs"])),
        "expected camelCase testFiles field, got {populated_value:?}"
    );
    let roundtrip_populated: AuditFixGroup = serde_json::from_value(populated_value)?;
    anyhow::ensure!(roundtrip_populated.test_files == vec!["tests/lib_test.rs"]);
    Ok(())
}
