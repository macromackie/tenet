use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, anyhow};
use serde_json::json;
use tenet_contracts::is_kebab;
use tenet_engine::discover_contracts;

use super::{INIT_VERSION, normalize_scope, write_credential_hint};
use crate::{cli::ExitStatus, error::CommandError, output, report_format::format_marker};

fn build_starter_document(name: &str) -> String {
    format!(
        "---\nname: {name}\nmessage: Preserve core project invariants in this scope.\n---\n\n## \
         Rules\n\nReplace this starter rule with one concrete invariant that callers and \
         reviewers can verify in code.\nState what must hold across changes, which edge cases \
         matter, and what behaviors must not regress.\n\n## Checks\n\nRun `tenet validate` after \
         editing this contract, then run `tenet check` to verify the governed files.\n"
    )
}

fn write_starter_result(
    out: &mut impl Write,
    name: &str,
    path: &Path,
    created: bool,
    json: bool,
) -> Result<(), CommandError> {
    if json {
        output::write_document(
            out,
            INIT_VERSION,
            json!({
                "mode": "starter",
                "created": created,
                "path": path,
                "name": name,
            }),
        )?;
        return Ok(());
    }
    if !created {
        writeln!(
            out,
            "  {} Contract `{name}` already exists at {}",
            format_marker("PASS"),
            path.display()
        )?;
        return Ok(());
    }
    writeln!(
        out,
        "  {} Created starter contract  {}",
        format_marker("PASS"),
        path.display()
    )?;
    writeln!(
        out,
        "\n  {}  Edit {}, then run `tenet validate` and `tenet check`.",
        output::format_styled("Next").bold(),
        path.display(),
    )?;
    write_credential_hint(out)?;
    Ok(())
}

pub(super) fn run_starter(
    root: &Path,
    scope: &Path,
    name: &str,
    json: bool,
) -> Result<ExitStatus, CommandError> {
    if !is_kebab(name) {
        return Err(anyhow!(
            "starter contract name {name:?} must be lowercase kebab-case (for example \
             \"project-invariants\")"
        )
        .into());
    }
    let relative_scope = normalize_scope(root, scope)?;
    let existing = discover_contracts(root)?;
    let mut out = output::open_stdout().lock();
    if let Some(found) = existing.iter().find(|contract| contract.name == name) {
        write_starter_result(&mut out, &found.name, &found.path, false, json)?;
        return Ok(ExitStatus::Passed);
    }
    let next_number = existing
        .iter()
        .filter(|contract| contract.scope == relative_scope)
        .map(|contract| contract.number)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let relative_path: PathBuf = relative_scope
        .join(".contracts")
        .join(format!("{next_number:03}-{name}"))
        .join("CONTRACT.md");
    let full_path = root.join(&relative_path);
    let parent = full_path
        .parent()
        .context("starter contract path has a parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("create {:?}", parent.display()))?;
    fs::write(&full_path, build_starter_document(name))
        .with_context(|| format!("write {:?}", full_path.display()))?;
    write_starter_result(&mut out, name, &relative_path, true, json)?;
    Ok(ExitStatus::Passed)
}
