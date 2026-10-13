use std::{
    io::{self, Write},
    path::Path,
};

use anyhow::Result;
use similar::TextDiff;
use tenet_engine::adoption::{
    self, AddRequest, Change, Destination, LocalState, Outcome, Status, Updated,
};

use crate::cli::Adoption;

/// Run an adoption command. Exit 1 means the user has something to resolve: an update, a conflict, or a removal.
pub(crate) fn execute(root: &Path, command: &Adoption) -> Result<u8> {
    let mut out = io::stdout();
    match command {
        Adoption::Add {
            spec,
            scope,
            path,
            name,
            number,
            sources,
        } => {
            let destination = match (scope, path) {
                (_, Some(path)) => Destination::Existing(path),
                (Some(scope), None) => Destination::Scope {
                    scope,
                    name: name.as_deref(),
                    number: *number,
                },
                (None, None) => anyhow::bail!("add requires --scope or --path"),
            };
            let added = adoption::add(
                root,
                &AddRequest {
                    spec,
                    destination,
                    sources,
                },
            )?;
            let verb = if added.written { "added" } else { "recorded" };
            writeln!(
                out,
                "{verb} {} from {}/{} {}",
                added.path.display(),
                added.source,
                added.contract,
                added.reference
            )?;
            writeln!(out, "next: tenet check --contract {}", added.name)?;
            Ok(0)
        }
        Adoption::Outdated { offline, json } => {
            let statuses = adoption::outdated(root, *offline)?;
            if *json {
                writeln!(
                    out,
                    "{}",
                    serde_json::json!({"version": 1, "contracts": statuses})
                )?;
            } else {
                for status in &statuses {
                    writeln!(out, "{}", status_line(status))?;
                }
            }
            Ok(u8::from(statuses.iter().any(Status::needs_attention)))
        }
        Adoption::Update {
            paths,
            reference,
            json,
        } => {
            let updated = adoption::update(root, paths, reference.as_deref())?;
            if *json {
                writeln!(
                    out,
                    "{}",
                    serde_json::json!({"version": 1, "contracts": updated})
                )?;
            } else {
                for update in &updated {
                    writeln!(out, "{}", update_line(update))?;
                }
            }
            Ok(u8::from(updated.iter().any(|u| {
                matches!(u.outcome, Outcome::Conflict { .. } | Outcome::Removed)
            })))
        }
        Adoption::Diff { path, json } => {
            let comparison = adoption::diff(root, path)?;
            if *json {
                writeln!(
                    out,
                    "{}",
                    serde_json::json!({"version": 1, "contract": comparison})
                )?;
                return Ok(0);
            }
            let base = &comparison.base;
            match &comparison.upstream {
                Some(upstream) => write!(
                    out,
                    "{}",
                    TextDiff::from_lines(&base.text, &upstream.text)
                        .unified_diff()
                        .header(
                            &format!("upstream {}", base.reference),
                            &format!("upstream {}", upstream.reference)
                        )
                )?,
                None => writeln!(out, "upstream removed this contract")?,
            }
            write!(
                out,
                "{}",
                TextDiff::from_lines(&base.text, &comparison.local)
                    .unified_diff()
                    .header(&format!("upstream {}", base.reference), "local")
            )?;
            Ok(0)
        }
        Adoption::Remove { path, keep_file } => {
            let path = adoption::remove(root, path, *keep_file)?;
            let verb = if *keep_file { "untracked" } else { "removed" };
            writeln!(out, "{verb} {}", path.display())?;
            Ok(0)
        }
    }
}

fn status_line(status: &Status) -> String {
    let state = match status.state {
        LocalState::Unchanged => "unchanged",
        LocalState::Edited => "edited",
        LocalState::Missing => "missing",
    };
    let version = match &status.newest {
        None => status.reference.clone(),
        Some(newest) => match newest.change {
            Change::Current => status.reference.clone(),
            Change::Changed => format!("{} -> {}", status.reference, newest.reference),
            Change::Removed => format!("{} -> removed in {}", status.reference, newest.reference),
        },
    };
    format!(
        "{}  {}  {}/{}  {version}  {state}",
        status.path.display(),
        status.name.as_deref().unwrap_or("-"),
        status.source,
        status.contract
    )
}

fn update_line(update: &Updated) -> String {
    let outcome = match update.outcome {
        Outcome::Unchanged => "unchanged".to_owned(),
        Outcome::Replaced => "replaced".to_owned(),
        Outcome::Merged => "merged".to_owned(),
        Outcome::Conflict { regions: 1 } => "conflict (1 region)".to_owned(),
        Outcome::Conflict { regions } => format!("conflict ({regions} regions)"),
        Outcome::Removed => "removed upstream".to_owned(),
    };
    format!(
        "{outcome}  {}  {} -> {}",
        update.path.display(),
        update.from,
        update.to
    )
}
