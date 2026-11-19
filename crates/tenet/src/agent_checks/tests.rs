use std::{fs, io};

use serde_json::json;

use super::{Outcome, process, request};

#[test]
fn embedded_worker_preserves_a_context_limit_without_provider_access() -> anyhow::Result<()> {
    let checkout = tempfile::tempdir()?;
    let root = checkout.path();
    let path = ".contracts/001-options/CONTRACT.md";
    fs::create_dir_all(root.join(".contracts/001-options"))?;
    fs::write(root.join(path), "Preserve explicit options.")?;
    fs::write(root.join("large.ts"), vec![b'x'; 60_000])?;
    let request = json!({
        "root": root,
        "state": root.join(".tenet"),
        "contracts": [{"name":"options", "hash":"frozen", "document":"Preserve explicit options.",
                       "path":path, "paths":["large.ts"]}],
        "maxGenerations": 0_usize,
    });
    let rows = process::execute(&request, root, false)?;
    anyhow::ensure!(
        rows.iter()
            .any(|row| row.check == "coverage" && row.outcome == Outcome::Unresolved)
    );
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
    let input = request::build_contract_input(&plan, contract, None, &[]);
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
