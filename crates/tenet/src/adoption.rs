use std::{io::Write as _, path::Path};

use similar::TextDiff;
use tenet_engine::adoption::{
    self,
    Added,
    Change,
    Comparison,
    FileComparison,
    Kind,
    LocalState,
    Outcome,
    Status,
    Updated,
};

use crate::{
    cli::{ExitStatus, Locked},
    error::CommandError,
    output,
};

/// The schema version of the objects that `outdated --json`, `update --json` and `diff --json`
/// print, for contracts and skills alike.
const VERSION: u64 = 1;

/// Print what `add` installed or recorded.
pub(crate) fn report_added(added: &Added, next: &str) -> Result<ExitStatus, CommandError> {
    let mut out = output::open_stdout();
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
    Ok(ExitStatus::Passed)
}

/// Run a command on recorded copies. A failed status means the user has something to resolve: an
/// update, an edited read-only file, a conflict, or a removal.
#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
pub(crate) fn execute(
    root: &Path,
    kind: Kind,
    command: &Locked,
) -> Result<ExitStatus, CommandError> {
    let mut out = output::open_stdout();
    match command {
        Locked::Outdated { offline, json } => {
            let statuses = adoption::list_outdated(root, kind, *offline)?;
            if *json {
                output::write_document(
                    &mut out,
                    VERSION,
                    serde_json::json!({kind.name(): statuses}),
                )?;
            } else {
                for status in &statuses {
                    write!(out, "{}", format_status_lines(status))?;
                }
            }
            Ok(if statuses.iter().any(Status::needs_attention) {
                ExitStatus::Failed
            } else {
                ExitStatus::Passed
            })
        },
        Locked::Update {
            paths,
            reference,
            json,
        } => {
            let updated = adoption::update(root, kind, paths, reference.as_deref())?;
            if *json {
                output::write_document(
                    &mut out,
                    VERSION,
                    serde_json::json!({kind.name(): updated}),
                )?;
            } else {
                for update in &updated {
                    write!(out, "{}", update_lines(update))?;
                }
            }
            Ok(if updated.iter().any(Updated::needs_attention) {
                ExitStatus::Failed
            } else {
                ExitStatus::Passed
            })
        },
        Locked::Diff { path, json } => {
            let comparison = adoption::diff(root, kind, path)?;
            let singular = match kind {
                Kind::Contracts => "contract",
                Kind::Skills => "skill",
            };
            if *json {
                output::write_document(
                    &mut out,
                    VERSION,
                    serde_json::json!({singular: comparison}),
                )?;
            } else {
                write!(out, "{}", diff_text(&comparison, singular))?;
            }
            Ok(ExitStatus::Passed)
        },
        Locked::Remove { path, keep_file } => {
            let path = adoption::remove(root, kind, path, *keep_file)?;
            let verb = if *keep_file { "untracked" } else { "removed" };
            writeln!(out, "{verb} {}", path.display())?;
            Ok(ExitStatus::Passed)
        },
    }
}

#[expect(
    clippy::format_push_string,
    reason = "The text is built in memory, where `write!` would add a Result that cannot fail."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
fn format_status_lines(status: &Status) -> String {
    let state = |state: LocalState| {
        match state {
            LocalState::Unchanged => "unchanged",
            LocalState::Edited => "edited",
            LocalState::Missing => "missing",
        }
    };
    let change = |change: Change| {
        match change {
            Change::Current => "current upstream",
            Change::Changed => "changed upstream",
            Change::Added => "added upstream",
            Change::Removed => "removed upstream",
        }
    };
    let version = match &status.newest {
        None => status.reference.clone(),
        Some(newest) => {
            let majors = match newest.majors.as_slice() {
                [] => String::new(),
                [_] => " (crosses 1 major)".to_owned(),
                [next, ..] => {
                    format!(
                        " (crosses {} majors; update stops at {next})",
                        newest.majors.len()
                    )
                },
            };
            match newest.change {
                Change::Current if newest.files.is_empty() => status.reference.clone(),
                Change::Removed => {
                    format!(
                        "{} -> removed in {}{majors}",
                        status.reference, newest.reference
                    )
                },
                Change::Current | Change::Changed | Change::Added => {
                    format!("{} -> {}{majors}", status.reference, newest.reference)
                },
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

#[expect(
    clippy::format_push_string,
    reason = "The text is built in memory, where `write!` would add a Result that cannot fail."
)]
fn update_lines(update: &Updated) -> String {
    let outcome = |outcome: Outcome| {
        match outcome {
            Outcome::Unchanged => "unchanged".to_owned(),
            Outcome::Replaced => "replaced".to_owned(),
            Outcome::Merged => "merged".to_owned(),
            Outcome::Conflict { regions: 1 } => "conflict (1 region)".to_owned(),
            Outcome::Conflict { regions } => format!("conflict ({regions} regions)"),
            Outcome::Added => "added".to_owned(),
            Outcome::Removed => "removed upstream".to_owned(),
        }
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
#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
fn diff_text(comparison: &Comparison, singular: &str) -> String {
    let base = &comparison.base;
    let upstream = comparison.upstream.as_ref();
    let mut text = match upstream {
        Some(upstream) => {
            TextDiff::from_lines(&base.text, &upstream.text)
                .unified_diff()
                .header(
                    &format!("upstream {}", base.reference),
                    &format!("upstream {}", upstream.reference),
                )
                .to_string()
        },
        None => format!("upstream removed this {singular}\n"),
    };
    text += &TextDiff::from_lines(&base.text, &comparison.local)
        .unified_diff()
        .header(&format!("upstream {}", base.reference), "local")
        .to_string();
    for file in &comparison.files {
        text += &format_file_diff(
            file,
            &base.reference,
            upstream.map(|upstream| upstream.reference.as_str()),
        );
    }
    text
}

#[expect(
    clippy::format_push_string,
    reason = "The text is built in memory, where `write!` would add a Result that cannot fail."
)]
fn format_file_diff(file: &FileComparison, base: &str, upstream: Option<&str>) -> String {
    let path = file.path.display();
    let before = file.base.as_deref().unwrap_or_default();
    let mut text = match (&file.upstream, upstream) {
        (Some(next), Some(reference)) => {
            TextDiff::from_lines(before, next.as_str())
                .unified_diff()
                .header(
                    &format!("upstream {base} {path}"),
                    &format!("upstream {reference} {path}"),
                )
                .to_string()
        },
        _ => format!("upstream removed {path}\n"),
    };
    text += &match (&file.local, &file.base) {
        (Some(local), _) => {
            TextDiff::from_lines(before, local.as_str())
                .unified_diff()
                .header(&format!("upstream {base} {path}"), &format!("local {path}"))
                .to_string()
        },
        (None, Some(_)) => format!("local {path} is missing\n"),
        (None, None) => String::new(),
    };
    text
}
