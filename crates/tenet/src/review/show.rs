use std::{
    collections::HashSet,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use serde_json::{Value, json};
use tenet_engine::{
    Change,
    Review,
    review::{self, Assessment, CurrentContract},
};

use super::{HISTORY, History, SHOW_VERSION, write_brief};
use crate::{cli::ExitStatus, error::CommandError, output};

#[expect(
    clippy::indexing_slicing,
    reason = "`value` is a JSON object the function builds, so the key assignment inserts a field."
)]
#[expect(
    clippy::future_not_send,
    reason = "The command drives this future on one current-thread runtime and never moves it \
              across threads."
)]
pub(crate) async fn show(
    root: &Path,
    state: &Path,
    name: &str,
    paths: &[PathBuf],
    json: bool,
) -> Result<ExitStatus, CommandError> {
    let review = Review::open_existing(state)?;
    let current = review::resolve_current(root, &review, None, Some(name)).await?;
    let entry = current.select(name, paths)?;
    let mut out = BufWriter::new(output::open_stdout().lock());
    if !json {
        write_brief(&mut out, &review, &current, entry)?;
        change(&mut out, &entry.changes)?;
        out.flush()?;
        return Ok(ExitStatus::Passed);
    }

    let files: Vec<_> = entry
        .files
        .iter()
        .map(|scoped| {
            let patch = entry
                .changes
                .iter()
                .find(|change| change.path == scoped.file.path)
                .map(|change| change.patch.as_str())
                .filter(|patch| !patch.is_empty());
            let mut value = serde_json::to_value(scoped)?;
            value["patch"] = json!(patch);
            Ok(value)
        })
        .collect::<Result<_, CommandError>>()?;
    output::write_document(
        &mut out,
        SHOW_VERSION,
        render_contract_json(&review, entry, &files)?,
    )?;
    out.flush()?;
    Ok(ExitStatus::Passed)
}

/// One contract as `show --json` reports it, with `files` as given.
#[expect(
    clippy::indexing_slicing,
    reason = "The slice end is at most the records' length."
)]
fn render_contract_json(
    review: &Review,
    entry: &CurrentContract<'_>,
    files: &[Value],
) -> Result<Value, CommandError> {
    let history = History::new(review, entry)?;
    let shown = |assessment: &Assessment| {
        json!({
            "rev": assessment.rev,
            "input": assessment.input,
            "scope": assessment.scope,
            "contract_hash": assessment.contract_hash,
            "conclusion": assessment.conclusion,
            "carried_from": assessment.carried_from,
            "results": assessment.results,
        })
    };
    let (records, earlier) = (&history.records, &history.earlier);
    let contract = &entry.contract;
    Ok(json!({
        "contract": {
            "name": contract.name,
            "message": contract.message,
            "path": contract.path,
            "scope": contract.scope,
            "hash": contract.hash,
        },
        "document": contract.document,
        "state": entry.state,
        "selection": {"rev": entry.selection.rev, "paths": entry.selection.paths},
        "mode": entry.mode,
        "scope": entry.scope,
        "input": entry.input,
        "min_confidence": entry.min_confidence,
        "assessment": entry.assessment.map(shown),
        "files": files,
        "records": &records[..records.len().min(HISTORY)],
        "earlier": earlier.iter().take(HISTORY).map(|prior| shown(prior)).collect::<Vec<_>>(),
        "older": {
            "records": records.len().saturating_sub(HISTORY),
            "assessments": earlier.len().saturating_sub(HISTORY),
        },
        "changes_since_earlier": history.changes_since,
        "conclusions": review::list_conclusions(entry.mode, entry.scope),
    }))
}

/// Every pending contract's brief, then the change they share, printed once. As JSON, each
/// contract as `show --json` reports it without patches, then each changed file's patch once.
#[expect(
    clippy::future_not_send,
    reason = "The command drives this future on one current-thread runtime and never moves it \
              across threads."
)]
#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
pub(crate) async fn show_pending(
    root: &Path,
    state: &Path,
    json: bool,
) -> Result<ExitStatus, CommandError> {
    let review = Review::open_existing(state)?;
    let current = review::resolve_current(root, &review, None, None).await?;
    let pending: Vec<_> = (current.contracts.iter())
        .filter(|entry| entry.state.is_pending())
        .collect();
    let mut seen_paths = HashSet::new();
    let changes: Vec<_> = (pending.iter())
        .flat_map(|entry| &entry.changes)
        .filter(|change| seen_paths.insert(&change.path))
        .cloned()
        .collect();
    let mut out = BufWriter::new(output::open_stdout().lock());
    if json {
        let contracts = (pending.iter())
            .map(|entry| {
                render_contract_json(
                    &review,
                    entry,
                    &(entry.files.iter())
                        .map(serde_json::to_value)
                        .collect::<Result<Vec<_>, _>>()?,
                )
            })
            .collect::<Result<Vec<_>, CommandError>>()?;
        let changes: Vec<_> = (changes.iter())
            .filter(|change| !change.patch.is_empty())
            .map(|change| json!({"path": change.path, "patch": change.patch}))
            .collect();
        output::write_document(
            &mut out,
            SHOW_VERSION,
            json!({"contracts": contracts, "changes": changes}),
        )?;
        out.flush()?;
        return Ok(ExitStatus::Passed);
    }
    if pending.is_empty() {
        writeln!(out, "No contract is pending.")?;
    }
    for (index, entry) in pending.iter().enumerate() {
        if index > 0 {
            writeln!(out, "\n{}\n", "\u{2500}".repeat(72))?;
        }
        write_brief(&mut out, &review, &current, entry)?;
    }
    change(&mut out, &changes)?;
    out.flush()?;
    Ok(ExitStatus::Passed)
}

/// Each file's patch, under a Change heading when there is any.
fn change(out: &mut impl Write, changes: &[Change]) -> Result<(), CommandError> {
    let patches: Vec<_> = changes
        .iter()
        .filter(|change| !change.patch.is_empty())
        .collect();
    if !patches.is_empty() {
        writeln!(out, "\nChange")?;
    }
    for change in patches {
        writeln!(
            out,
            "\n--- {}\n{}",
            change.path.display(),
            change.patch.trim_end()
        )?;
    }
    Ok(())
}
