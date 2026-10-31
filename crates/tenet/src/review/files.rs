use std::io::Write;

use tenet_engine::{
    Mode,
    review::{Current, CurrentContract, Role},
};

use super::{LISTED, ROUTINE_KINDS, UNREADABLE, format_row_name, format_tally};
use crate::error::CommandError;

/// List scoped files while keeping unreadable and supporting inputs visible.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Counts are bounded by the scoped input files."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The listing shares unreadable counts with its displayed rows and final summary."
)]
pub(super) fn write_files(
    out: &mut impl Write,
    current: &Current<'_>,
    entry: &CurrentContract<'_>,
) -> Result<(), CommandError> {
    // A long scope lists only files that need attention: support, unreadable, and in a diff, those
    // the Change section shows no patch for. Unreadable files past the first few are counted.
    let kinds: Vec<_> = entry
        .files
        .iter()
        .map(|scoped| {
            let file = &scoped.file;
            let kind = match (scoped.role, &file.before, &file.after) {
                (Role::Support, ..) => "support",
                _ if file.error.is_some() => "unreadable",
                _ if entry.mode == Mode::Full => "selected",
                (_, None, _) => "added",
                (_, _, None) => "deleted",
                _ if file.previous_path.is_some() => "renamed",
                _ => "modified",
            };
            (kind, scoped)
        })
        .collect();
    writeln!(
        out,
        "\nFiles  {}",
        format_tally(kinds.iter().map(|(kind, _)| *kind))
    )?;
    let listed = kinds
        .iter()
        .filter(|(kind, _)| kinds.len() <= LISTED || !ROUTINE_KINDS.contains(kind));
    let mut unreadable = 0;
    for (kind, scoped) in listed {
        if *kind == "unreadable" {
            unreadable += 1;
            if unreadable > UNREADABLE {
                continue;
            }
        }
        let excerpt = match &*scoped.file.excerpt {
            [] => String::new(),
            lines => format!(" lines {}", tenet_engine::format_line_ranges(lines)),
        };
        writeln!(out, "  {kind:10} {}{excerpt}", scoped.file.path.display())?;
        if let Some(error) = &scoped.file.error {
            writeln!(out, "             {error}")?;
        }
    }
    if unreadable > UNREADABLE {
        writeln!(out, "  and {} more unreadable", unreadable - UNREADABLE)?;
    }
    if kinds.len() > LISTED || unreadable > UNREADABLE {
        writeln!(
            out,
            "  tenet show {} --json lists every file",
            format_row_name(current, entry)
        )?;
    }

    Ok(())
}
