use std::{
    io::{self, Write},
    path::Path,
};

use anyhow::Result;
use similar::TextDiff;
use tenet_engine::adoption::{
    self, Added, Change, Comparison, FileComparison, Kind, LocalState, Outcome, Status, Updated,
};

use crate::cli::Locked;

/// Print what `add` installed or recorded.
pub(crate) fn added(added: &Added, next: &str) -> Result<u8> {
    let mut out = io::stdout();
    let verb = if added.written { "added" } else { "recorded" };
    writeln!(
        out,
        "{verb} {} from {}/{} {}",
        added.path.display(),
        added.source,
        added.origin.name(),
        added.reference
    )?;
    writeln!(out, "next: {next}")?;
    Ok(0)
}

/// Run a command on recorded copies. Exit 1 means the user has something to resolve: an update,
/// an edited read-only file, a conflict, or a removal.
pub(crate) fn execute(root: &Path, kind: Kind, command: &Locked) -> Result<u8> {
    let mut out = io::stdout();
    match command {
        Locked::Outdated { offline, json } => {
            let statuses = adoption::outdated(root, kind, *offline)?;
            if *json {
                writeln!(
                    out,
                    "{}",
                    serde_json::json!({"version": 1, kind.name(): statuses})
                )?;
            } else {
                for status in &statuses {
                    write!(out, "{}", status_lines(status))?;
                }
            }
            Ok(u8::from(statuses.iter().any(Status::needs_attention)))
        }
        Locked::Update {
            paths,
            reference,
            json,
        } => {
            let updated = adoption::update(root, kind, paths, reference.as_deref())?;
            if *json {
                writeln!(
                    out,
                    "{}",
                    serde_json::json!({"version": 1, kind.name(): updated})
                )?;
            } else {
                for update in &updated {
                    write!(out, "{}", update_lines(update))?;
                }
            }
            Ok(u8::from(updated.iter().any(Updated::needs_attention)))
        }
        Locked::Diff { path, json } => {
            let comparison = adoption::diff(root, kind, path)?;
            let singular = match kind {
                Kind::Contracts => "contract",
                Kind::Skills => "skill",
            };
            if *json {
                writeln!(
                    out,
                    "{}",
                    serde_json::json!({"version": 1, singular: comparison})
                )?;
            } else {
                write!(out, "{}", diff_text(&comparison, singular))?;
            }
            Ok(0)
        }
        Locked::Remove { path, keep_file } => {
            let path = adoption::remove(root, kind, path, *keep_file)?;
            let verb = if *keep_file { "untracked" } else { "removed" };
            writeln!(out, "{verb} {}", path.display())?;
            Ok(0)
        }
    }
}

fn status_lines(status: &Status) -> String {
    let state = |state: LocalState| match state {
        LocalState::Unchanged => "unchanged",
        LocalState::Edited => "edited",
        LocalState::Missing => "missing",
    };
    let change = |change: Change| match change {
        Change::Current => "current upstream",
        Change::Changed => "changed upstream",
        Change::Added => "added upstream",
        Change::Removed => "removed upstream",
    };
    let version = match &status.newest {
        None => status.reference.clone(),
        Some(newest) => match newest.change {
            Change::Current if newest.files.is_empty() => status.reference.clone(),
            Change::Removed => format!("{} -> removed in {}", status.reference, newest.reference),
            Change::Current | Change::Changed | Change::Added => {
                format!("{} -> {}", status.reference, newest.reference)
            }
        },
    };
    let mut lines = format!(
        "{}  {}  {}/{}  {version}  {}\n",
        status.path.display(),
        status.name.as_deref().unwrap_or("-"),
        status.source,
        status.origin.name(),
        state(status.state)
    );
    for (path, file) in &status.files {
        lines += &format!("  {}  {}\n", path.display(), state(*file));
    }
    for (path, file) in status.newest.iter().flat_map(|newest| &newest.files) {
        lines += &format!("  {}  {}\n", path.display(), change(*file));
    }
    lines
}

fn update_lines(update: &Updated) -> String {
    let outcome = |outcome: Outcome| match outcome {
        Outcome::Unchanged => "unchanged".to_owned(),
        Outcome::Replaced => "replaced".to_owned(),
        Outcome::Merged => "merged".to_owned(),
        Outcome::Conflict { regions: 1 } => "conflict (1 region)".to_owned(),
        Outcome::Conflict { regions } => format!("conflict ({regions} regions)"),
        Outcome::Added => "added".to_owned(),
        Outcome::Removed => "removed upstream".to_owned(),
    };
    let mut lines = format!(
        "{}  {}  {} -> {}\n",
        outcome(update.outcome),
        update.path.display(),
        update.from,
        update.to
    );
    for (path, file) in &update.files {
        lines += &format!("  {}  {}\n", outcome(*file), path.display());
    }
    lines
}

/// Upstream changes since the locked release, then local edits, for the CONTRACT.md or SKILL.md
/// and each other file that differs.
fn diff_text(comparison: &Comparison, singular: &str) -> String {
    let base = &comparison.base;
    let upstream = comparison.upstream.as_ref();
    let mut text = match upstream {
        Some(upstream) => TextDiff::from_lines(&base.text, &upstream.text)
            .unified_diff()
            .header(
                &format!("upstream {}", base.reference),
                &format!("upstream {}", upstream.reference),
            )
            .to_string(),
        None => format!("upstream removed this {singular}\n"),
    };
    text += &TextDiff::from_lines(&base.text, &comparison.local)
        .unified_diff()
        .header(&format!("upstream {}", base.reference), "local")
        .to_string();
    for file in &comparison.files {
        text += &file_diff(
            file,
            &base.reference,
            upstream.map(|upstream| upstream.reference.as_str()),
        );
    }
    text
}

fn file_diff(file: &FileComparison, base: &str, upstream: Option<&str>) -> String {
    let path = file.path.display();
    let before = file.base.as_deref().unwrap_or_default();
    let mut text = match (&file.upstream, upstream) {
        (Some(next), Some(reference)) => TextDiff::from_lines(before, next.as_str())
            .unified_diff()
            .header(
                &format!("upstream {base} {path}"),
                &format!("upstream {reference} {path}"),
            )
            .to_string(),
        _ => format!("upstream removed {path}\n"),
    };
    text += &match (&file.local, &file.base) {
        (Some(local), _) => TextDiff::from_lines(before, local.as_str())
            .unified_diff()
            .header(&format!("upstream {base} {path}"), &format!("local {path}"))
            .to_string(),
        (None, Some(_)) => format!("local {path} is missing\n"),
        (None, None) => String::new(),
    };
    text
}
