use std::io::Write;

use serde_json::Value;
use tenet_engine::{
    ContractState,
    Mode,
    Pending,
    Review,
    Status,
    review::{self, Current, CurrentContract, Role},
};

use super::{
    CITATIONS,
    HISTORY,
    History,
    LISTED,
    SETTLED_STATUSES,
    check_command,
    format_ago,
    format_paths_note,
    format_row_name,
    format_span_text,
    format_state_text,
    format_tally,
    format_threshold_note,
    write_files,
};
use crate::{
    error::CommandError,
    report_format::{self, format_violation},
};

/// What a reviewer needs to settle one contract, everything but the change itself: its state,
/// the model's answer, the scoped files, history, the record command, and the contract.
#[expect(
    clippy::indexing_slicing,
    reason = "The slice end is at most the citations' length."
)]
#[expect(
    clippy::cognitive_complexity,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
#[expect(
    clippy::else_if_without_else,
    reason = "`needless_else` rejects the empty `else` this lint asks for."
)]
#[expect(
    clippy::excessive_nesting,
    reason = "Each level is a loop, match or closure over one parsed structure, and flattening it \
              would split one decision."
)]
#[expect(
    clippy::shadow_unrelated,
    reason = "Each block binds its own result under the name the following lines read."
)]
#[expect(
    clippy::string_slice,
    reason = "Each slice bound is a character boundary: an ASCII hex hash prefix, a str::find \
              offset, or a span the parser returns."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
pub(super) fn write_brief(
    out: &mut impl Write,
    review: &Review,
    current: &Current<'_>,
    entry: &CurrentContract<'_>,
) -> Result<(), CommandError> {
    let name = &entry.contract.name;
    let History {
        records,
        earlier,
        changes_since,
    } = History::new(review, entry)?;
    let (label, note) = format_state_text(entry);
    writeln!(out, "{name}  {}  {note}", label.to_lowercase())?;
    writeln!(out, "  {}", entry.contract.path.display())?;
    writeln!(
        out,
        "  Review {}{} · selection checked in r{}",
        entry.selection.review,
        format_paths_note(entry.selection),
        entry.selection.rev
    )?;
    if let Some(assessment) = entry.assessment {
        writeln!(
            out,
            "  Assessed in r{} · {} check of {} · input {}{}",
            assessment.rev,
            entry.mode.as_str(),
            entry.scope.describe(),
            &entry.input[..12],
            format_threshold_note(entry.min_confidence)
        )?;
    }
    if matches!(
        entry.state,
        ContractState::Pending {
            reason: Pending::ContractChanged
        }
    ) {
        writeln!(
            out,
            "\n  This change adds or edits the contract, so the diff check's assumption that the \
             base\n  complied with it does not hold. Assess the whole contract in a separate full \
             review\n  with `tenet check --contract {name}`, then return with `{}`\n  and record \
             a conclusion here.",
            check_command(entry.selection, entry.selection.contract.as_deref())
        )?;
    }

    if let Some(assessment) = entry.assessment {
        writeln!(out, "\nModel")?;
        match &assessment.conclusion {
            Some(conclusion) => {
                writeln!(
                    out,
                    "  {}{}: {}",
                    conclusion.status.as_str(),
                    format_violation(conclusion.violation),
                    conclusion.reason
                )?;
                if let Some(error) = &conclusion.error {
                    writeln!(out, "  {error}")?;
                }
                if !conclusion.conflicting_files.is_empty() {
                    writeln!(
                        out,
                        "  Disagrees with file checks: {}",
                        conclusion.conflicting_files.join(", ")
                    )?;
                }
            },
            None => writeln!(out, "  not assessed")?,
        }
        let results = &assessment.results;
        if results.len() > LISTED {
            let statuses = results
                .iter()
                .map(|result| report_format::label_status(result.status));
            writeln!(out, "  File checks: {}", format_tally(statuses))?;
        }
        let attention = |status: Status| !SETTLED_STATUSES.contains(&status);
        for result in results
            .iter()
            .filter(|result| results.len() <= LISTED || attention(result.status))
        {
            let omitted = report_format::format_omitted(&result.omitted);
            writeln!(
                out,
                "  {}  {}{}{}",
                result.path,
                report_format::label_status(result.status).to_lowercase(),
                format_violation(result.violation),
                if omitted.is_empty() {
                    omitted
                } else {
                    format!(" · {omitted}")
                }
            )?;
        }
    }

    write_files(out, current, entry)?;

    if !records.is_empty() || !earlier.is_empty() {
        writeln!(out, "\nHistory")?;
        for record in records.iter().take(HISTORY) {
            let currency = if record.input == entry.input {
                "current"
            } else {
                "outdated"
            };
            writeln!(
                out,
                "  record {}  {}  {}  {currency}\n    {}",
                record.id,
                record.conclusion.as_str(),
                format_ago(record.at),
                record.message.replace('\n', "\n    "),
            )?;
            if !record.evidence.is_empty() {
                let spans: Vec<_> = record.evidence.iter().map(format_span_text).collect();
                writeln!(out, "    {}", spans.join(", "))?;
            }
        }
        for assessment in earlier.iter().take(HISTORY) {
            writeln!(
                out,
                "  r{}  model {}  outdated",
                assessment.rev,
                assessment
                    .conclusion
                    .as_ref()
                    .map_or("not assessed", |conclusion| conclusion.status.as_str())
            )?;
        }
        let older = (
            records.len().saturating_sub(HISTORY),
            earlier.len().saturating_sub(HISTORY),
        );
        if older != (0, 0) {
            writeln!(
                out,
                "  {} older records and {} older assessments; tenet log {name} lists them",
                older.0, older.1
            )?;
        }
        if let Some(old) = earlier.first() {
            writeln!(out, "\nChanges since r{}", old.rev)?;
            let contract_path = entry.contract.path.display().to_string();
            for change in &changes_since {
                let path = change["path"].as_str().unwrap_or("");
                match (&change["diff"], &change["note"]) {
                    // In a diff review, Change below shows each file's whole change once.
                    (Value::String(_), _) if entry.mode == Mode::Diff && path != contract_path => {
                        writeln!(
                            out,
                            "  {path}: {} added, {} removed since r{}; see Change",
                            change["added"], change["removed"], old.rev
                        )?;
                    },
                    (Value::String(diff), _) => writeln!(out, "{}", diff.trim_end())?,
                    (_, note) => writeln!(out, "  {path}: {}", note.as_str().unwrap_or(""))?,
                }
            }
        }
    }

    writeln!(out, "\nRecord")?;
    let conclusions = review::list_conclusions(entry.mode, entry.scope);
    let choices: Vec<_> = conclusions
        .iter()
        .map(|conclusion| conclusion.as_str())
        .collect();
    writeln!(
        out,
        "  tenet record {} --conclusion {} --evidence PATH:START-END -m \"what the evidence \
         shows\"",
        format_row_name(current, entry),
        choices.join("|")
    )?;
    let flagged: Vec<_> = entry
        .assessment
        .iter()
        .flat_map(|assessment| &assessment.results)
        .filter(|result| result.status == Status::Fail)
        .map(|result| result.path.as_str())
        .collect();
    if !flagged.is_empty() {
        writeln!(
            out,
            "  A favorable conclusion must cite a file the model flagged: {}",
            flagged.join(", ")
        )?;
    }
    // The same rules `record` applies to a diff; see its refusals for the reasons.
    let selected = || (entry.files.iter()).filter(|scoped| scoped.role == Role::Selected);
    if entry.mode == Mode::Diff && selected().next().is_none() {
        writeln!(
            out,
            "  A favorable conclusion needs `tenet check --contract {name}` settled as verified \
             first"
        )?;
    } else if entry.mode == Mode::Diff && selected().any(|scoped| scoped.file.is_changed()) {
        let citations: Vec<_> = (entry.files.iter())
            .flat_map(|scoped| scoped.file.change_citations())
            .collect();
        let more = match citations.len().saturating_sub(CITATIONS) {
            0 => String::new(),
            more => format!(", and {more} more; see Change"),
        };
        writeln!(
            out,
            "  A favorable conclusion must cite a changed line, or one beside it: {}{more}",
            citations[..citations.len().min(CITATIONS)].join(", ")
        )?;
    }

    writeln!(out, "\nContract\n\n{}", entry.contract.document.trim_end())?;
    Ok(())
}
