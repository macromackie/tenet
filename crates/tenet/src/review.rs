//! Commands that read and extend the review kept in `.tenet/`: status, show, record, and log.

use alloc::collections::BTreeMap;
use std::{
    collections::{HashMap, HashSet},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context as _;
use serde_json::{Value, json};
use similar::{ChangeTag, TextDiff};
use tenet_engine::{
    Change,
    ContractState,
    ContractStatus,
    MIN_CONFIDENCE,
    Mode,
    Pending,
    Review,
    Status,
    review::{
        self,
        Assessment,
        Current,
        CurrentContract,
        Entry,
        MAX_BLOB_BYTES,
        Record,
        RecordRequest,
        Recorded,
        ReviewError,
        Revision,
        Role,
    },
};

use crate::{
    cli::{self, ExitStatus},
    error::CommandError,
    output,
    report_format,
    report_format::{Model, format_marker, format_painted, format_violation},
};

/// The schema version of the object that `status --json` prints.
const STATUS_VERSION: u64 = 1;
/// The schema version of the objects that `show NAME --json` and `show --pending --json` print.
const SHOW_VERSION: u64 = 2;
/// The schema version of each review log entry that `log --json` prints.
const LOG_VERSION: u64 = 2;

/// How many records and earlier assessments `show` lists; `log NAME` lists every one.
const HISTORY: usize = 5;
/// `show` lists every file and file result of a scope this size or smaller, and otherwise only
/// those that need attention.
const LISTED: usize = 20;
/// The results that need no attention.
const SETTLED_STATUSES: [Status; 2] = [Status::Pass, Status::NotApplicable];
/// The change kinds a long list leaves out.
const ROUTINE_KINDS: [&str; 2] = ["selected", "modified"];
/// How many unreadable files `show` names; it counts the rest.
const UNREADABLE: usize = 5;
/// How many changed ranges `show` offers as evidence.
const CITATIONS: usize = 10;

#[expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "Counts are bounded by the stored review's contracts, and `value` is a JSON object \
              the function builds."
)]
#[expect(
    clippy::else_if_without_else,
    reason = "`needless_else` rejects the empty `else` this lint asks for."
)]
#[expect(
    clippy::format_push_string,
    reason = "The text is built in memory, where `write!` would add a Result that cannot fail."
)]
#[expect(
    clippy::future_not_send,
    reason = "The command drives this future on one current-thread runtime and never moves it \
              across threads."
)]
#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
#[expect(
    clippy::shadow_unrelated,
    reason = "Each block binds its own result under the name the following lines read."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "The remaining variants take no action here, and listing each would repeat the enum."
)]
pub(crate) async fn report_status(
    root: &Path,
    state: &Path,
    base: Option<&str>,
    json: bool,
    prune_cache: bool,
) -> Result<ExitStatus, CommandError> {
    let review = Review::open_existing(state)?;
    if prune_cache {
        let pruned = review.prune_answers()?;
        writeln!(
            output::open_stderr(),
            "Removed {} of {} cached answers that this Tenet cannot reuse.",
            pruned.removed,
            pruned.removed + pruned.kept
        )?;
    }
    let mut out = output::open_stdout().lock();
    let current = match review::resolve_current(root, &review, base, None).await {
        Ok(current) => current,
        Err(ReviewError::Unchecked(unchecked)) => {
            // The review against this commit has not begun, so nothing in it is settled.
            if json {
                let message = unchecked.to_string();
                let base = &unchecked.base;
                output::write_document(
                    &mut out,
                    STATUS_VERSION,
                    json!({
                        "base": base,
                        "revision": null,
                        "contracts": [],
                        "exit_code": u8::from(ExitStatus::Pending),
                        "message": message,
                    }),
                )?;
            } else {
                writeln!(out, "{unchecked}")?;
            }
            return Ok(ExitStatus::Pending);
        },
        Err(error) => return Err(error.into()),
    };
    let status = ExitStatus::from(review::summarize_overall(
        current.contracts.iter().map(|contract| &contract.state),
    ));
    if json {
        let contracts = current
            .contracts
            .iter()
            .map(|entry| {
                let mut value = serde_json::to_value(&entry.state)?;
                value["contract"] = json!(entry.contract.name);
                value["path"] = json!(entry.contract.path);
                value["selection"] =
                    json!({"rev": entry.selection.rev, "paths": entry.selection.paths});
                value["input"] = json!(entry.input);
                value["assessed_in"] = json!(entry.assessment.map(|assessment| assessment.rev));
                value["model"] = json!(find_model(entry).map(|verdict| verdict.status));
                value["model_violation"] =
                    json!(find_model(entry).and_then(|verdict| verdict.violation));
                value["model_answer"] = json!(find_model(entry).and_then(|verdict| verdict.answer));
                value["min_confidence"] = json!(entry.min_confidence);
                Ok(value)
            })
            .collect::<Result<Vec<_>, CommandError>>()?;
        output::write_document(
            &mut out,
            STATUS_VERSION,
            json!({
                "review": current.latest.review,
                "base": current.latest.base_oid,
                "revision": current.latest.rev,
                "unchanged": current.unchanged,
                "contracts": contracts,
                "deleted_contracts": current.deleted,
                "exit_code": u8::from(status),
            }),
        )?;
        return Ok(status);
    }
    writeln!(
        out,
        "Review {}{} · latest check r{} {}\n",
        current.latest.review,
        format_paths_note(current.latest),
        current.latest.rev,
        format_ago(current.latest.at)
    )?;
    if current.unchanged {
        writeln!(
            out,
            " The working tree has no changes since the base; nothing to review."
        )?;
    }
    let width = current
        .contracts
        .iter()
        .map(|entry| entry.contract.name.len())
        .max()
        .unwrap_or(0);
    for entry in &current.contracts {
        let (label, mut note) = format_state_text(entry);
        if !entry.selection.is_selecting(&current.latest.paths) {
            note.push_str(&format!(
                " · from r{} {}",
                entry.selection.rev,
                format_selection_text(entry.selection)
            ));
        }
        writeln!(
            out,
            " {} {:width$}  {:10}  {note}",
            format_marker(label),
            entry.contract.name,
            format_painted(&label.to_lowercase()),
        )?;
    }
    for path in &current.deleted {
        writeln!(
            out,
            " {} {}  deleted by this change; its requirement no longer applies",
            format_marker("DELETED"),
            path.display()
        )?;
    }
    let pending: Vec<_> = current
        .contracts
        .iter()
        .filter(|contract| contract.state.is_pending())
        .collect();
    let recorded = current
        .contracts
        .iter()
        .filter(|contract| {
            matches!(
                contract.state,
                ContractState::Settled {
                    record: Some(_),
                    ..
                }
            )
        })
        .count();
    let settled = current.contracts.len() - pending.len();
    let by = if settled > 0 {
        format!(" ({} by model, {recorded} by record)", settled - recorded)
    } else {
        String::new()
    };
    writeln!(out, "\n {settled} settled{by} · {} pending", pending.len())?;
    let outdated = pending.iter().find(|contract| {
        matches!(
            contract.state,
            ContractState::Pending {
                reason: Pending::InputsChanged | Pending::NotChecked | Pending::NotAssessed
            }
        )
    });
    let violation = |contract: &CurrentContract<'_>| {
        find_model(contract)
            .and_then(|verdict| verdict.violation)
            .unwrap_or(-1.0_f64)
    };
    if let Some(outdated) = outdated {
        // A contract no check assessed needs a check of every contract, not of the one named.
        let contract = match outdated.state {
            ContractState::Pending {
                reason: Pending::NotChecked,
            } => None,
            _ => outdated.selection.contract.as_deref(),
        };
        writeln!(
            out,
            " Next  {}",
            check_command(outdated.selection, contract)
        )?;
    } else if let Some(first) = pending
        .iter()
        .max_by(|left, right| violation(left).total_cmp(&violation(right)))
    {
        writeln!(
            out,
            " Next  tenet show {}",
            format_row_name(&current, first)
        )?;
    }
    Ok(status)
}

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

/// A contract's records and earlier assessments, newest first, and what changed in its inputs
/// since the latest earlier assessment.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Fields follow the order the history shows them."
)]
struct History<'review> {
    records: Vec<&'review Record>,
    earlier: Vec<&'review Assessment>,
    changes_since: Vec<Value>,
}

impl<'review> History<'review> {
    fn new(review: &'review Review, entry: &CurrentContract<'_>) -> Result<Self, CommandError> {
        let name = &entry.contract.name;
        let records = (review.list_records().rev())
            .filter(|record| &record.contract == name)
            .collect();
        let mut inputs = HashSet::from([entry.input.as_str()]);
        let earlier: Vec<_> = (review.list_assessments().rev())
            .filter(|assessment| {
                &assessment.contract == name && inputs.insert(assessment.input.as_str())
            })
            .collect();
        let changes_since = earlier
            .first()
            .map(|old| compute_interdiff(review, old, entry))
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            records,
            earlier,
            changes_since,
        })
    }
}

/// What a reviewer needs to settle one contract, everything but the change itself: its state,
/// the model's answer, the scoped files, history, the record command, and the contract.
#[expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "The slice end is at most the citations' length, and the subtraction runs only when \
              the count exceeds the limit."
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
fn write_brief(
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

#[expect(
    clippy::future_not_send,
    reason = "The command drives this future on one current-thread runtime and never moves it \
              across threads."
)]
pub(crate) async fn record(
    root: &Path,
    state: &Path,
    args: &cli::Record,
) -> Result<ExitStatus, CommandError> {
    let conclusion = ContractStatus::parse(&args.conclusion).with_context(|| {
        format!(
            "unknown conclusion {:?}; use verified, preserved, unaffected, clear, failed, or \
             unresolved",
            args.conclusion
        )
    })?;
    let Recorded { record, replaces } = review::record(
        root,
        state,
        &RecordRequest {
            contract: args.contract.clone(),
            paths: args.paths.clone(),
            conclusion,
            evidence: args.evidence.clone(),
            message: args.message.clone(),
        },
    )
    .await?;
    let mut out = output::open_stdout().lock();
    writeln!(
        out,
        "Recorded {}: {} {} for r{}",
        record.id,
        record.contract,
        record.conclusion.as_str(),
        record.rev
    )?;
    if let Some(earlier) = replaces.filter(|earlier| earlier.conclusion != record.conclusion) {
        writeln!(
            out,
            "It replaces record {}, which concluded {} for this input.",
            earlier.id,
            earlier.conclusion.as_str()
        )?;
    }
    if let Some(model) = record.model
        && model != record.conclusion
    {
        writeln!(
            out,
            "The model concluded {}; this record decides the contract while its input is \
             unchanged.",
            model.as_str()
        )?;
    }
    Ok(ExitStatus::Passed)
}

#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
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
pub(crate) fn log(
    state: &Path,
    contract: Option<&str>,
    json: bool,
) -> Result<ExitStatus, CommandError> {
    let review = Review::open_existing(state)?;
    let shown = |name: &str| contract.is_none_or(|contract| contract == name);
    let mut out = BufWriter::new(output::open_stdout().lock());
    if json {
        for entry in review.entries() {
            let listed = match entry {
                Entry::Revision(revision) => {
                    review
                        .list_assessments_in(revision.rev)
                        .any(|assessment| shown(&assessment.contract))
                },
                Entry::Assessment(assessment) => shown(&assessment.contract),
                Entry::Record(record) => shown(&record.contract),
            };
            if listed {
                output::write_document(&mut out, LOG_VERSION, serde_json::to_value(entry)?)?;
            }
        }
        out.flush()?;
        return Ok(ExitStatus::Passed);
    }

    // A record names the one assessment its revision made of its contract.
    let mut records: HashMap<(u64, &str), Vec<&Record>> = HashMap::new();
    for record in review.list_records() {
        records
            .entry((record.rev, &record.contract))
            .or_default()
            .push(record);
    }
    let mut listed = false;
    for revision in review.list_revisions().rev() {
        let mut assessments = review
            .list_assessments_in(revision.rev)
            .filter(|assessment| shown(&assessment.contract))
            .peekable();
        if assessments.peek().is_none() {
            continue;
        }
        listed = true;
        writeln!(
            out,
            "r{}  {}  {}{}  {}{}",
            revision.rev,
            format_ago(revision.at),
            revision.review,
            format_paths_note(revision),
            revision.model.as_deref().unwrap_or("no model"),
            format_threshold_note(revision.min_confidence)
        )?;
        for assessment in assessments {
            let reused = assessment
                .carried_from
                .map(|rev| format!(" · reused from r{rev}"))
                .unwrap_or_default();
            writeln!(
                out,
                "  {}  model {}{reused}  input {}",
                assessment.contract,
                assessment
                    .conclusion
                    .as_ref()
                    .map_or("not assessed", |conclusion| conclusion.status.as_str()),
                &assessment.input[..12]
            )?;
            let made = records.get(&(revision.rev, assessment.contract.as_str()));
            for record in made.into_iter().flatten() {
                writeln!(
                    out,
                    "    record {}  {}  {}  {}",
                    record.id,
                    record.conclusion.as_str(),
                    format_ago(record.at),
                    record.message.replace('\n', "\n      ")
                )?;
            }
        }
    }
    if !listed {
        writeln!(out, "No checks recorded.")?;
    }
    out.flush()?;
    Ok(ExitStatus::Passed)
}

/// The model's conclusion for the row's exact input.
fn find_model(entry: &CurrentContract<'_>) -> Option<Model> {
    let conclusion = entry.assessment?.conclusion.as_ref()?;
    Some(Model {
        status: conclusion.status,
        violation: conclusion.violation,
        answer: conclusion.answer,
        min_confidence: entry.min_confidence,
    })
}

/// The row's state as `check` reports it, and which check the changed inputs were captured in.
#[expect(
    clippy::format_push_string,
    reason = "The text is built in memory, where `write!` would add a Result that cannot fail."
)]
fn format_state_text(entry: &CurrentContract<'_>) -> (&'static str, String) {
    let threshold = format_threshold_note(entry.min_confidence);
    let (label, mut note) =
        report_format::format_state(&entry.state, find_model(entry), &threshold);
    if entry.state
        == (ContractState::Pending {
            reason: Pending::InputsChanged,
        })
    {
        note.push_str(&format!(" after r{}", entry.selection.rev));
    }
    (label, note)
}

/// How many of each label, in label order: "2 fail · 30 pass".
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Counts are bounded by the contracts of one review."
)]
#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
fn format_tally<'label>(labels: impl Iterator<Item = &'label str>) -> String {
    let mut counts = BTreeMap::new();
    for label in labels {
        *counts.entry(label.to_lowercase()).or_insert(0_i32) += 1_i32;
    }
    let counts: Vec<_> = counts
        .iter()
        .map(|(label, count)| format!("{count} {label}"))
        .collect();
    counts.join(" \u{b7} ")
}

/// The name that selects this row in `show` and `record`: the contract, plus the selection's
/// paths when the contract stands in more than one check.
fn format_row_name(current: &Current<'_>, entry: &CurrentContract<'_>) -> String {
    let mut name = entry.contract.name.clone();
    let rows = current
        .contracts
        .iter()
        .filter(|contract| contract.contract.name == name)
        .count();
    if rows > 1 {
        for path in &entry.selection.paths {
            name.push(' ');
            name.push_str(&path.display().to_string());
        }
    }
    name
}

/// A revision's selected paths, as a note after the review's name.
fn format_paths_note(revision: &Revision) -> String {
    if revision.paths.is_empty() {
        String::new()
    } else {
        format!(" · {}", format_selection_text(revision))
    }
}

fn format_selection_text(revision: &Revision) -> String {
    if revision.paths.is_empty() {
        return "(every path)".into();
    }
    revision
        .paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A confidence threshold that differs from the default, which changes how answers are routed.
#[expect(
    clippy::float_cmp_const,
    reason = "The parser stores the default threshold unchanged, so an exact comparison finds it."
)]
fn format_threshold_note(min_confidence: Option<f64>) -> String {
    match min_confidence {
        Some(threshold) if threshold != MIN_CONFIDENCE => {
            format!(" · min confidence {threshold}")
        },
        _ => String::new(),
    }
}

/// What changed in the contract's inputs between an earlier assessment and now.
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
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
fn compute_interdiff(
    review: &Review,
    old: &Assessment,
    entry: &CurrentContract<'_>,
) -> Result<Vec<Value>, CommandError> {
    let mut changes = Vec::new();
    let path = entry.contract.path.display().to_string();
    let forgotten = format!(
        "contents from r{} are not in the review state, which keeps files up to {} MiB",
        old.rev,
        MAX_BLOB_BYTES >> 20_i32
    );
    if old.contract_hash != entry.contract.hash {
        match review.read_blob(&old.contract_hash)? {
            Some(read_before) => {
                let text = diff(&path, old.rev, &read_before, &entry.contract.document);
                changes.push(json!({"path": path, "diff": text}));
            },
            None => changes.push(json!({"path": path, "note": forgotten})),
        }
    }
    let revision = review.find_revision(old.rev)?;
    let previous_files: HashMap<_, _> = revision
        .list_files(old)
        .map(|(_, file)| (&file.path, file))
        .collect();
    for scoped in &entry.files {
        let file = &scoped.file;
        let path = file.path.display().to_string();
        let Some(previous) = previous_files.get(&file.path) else {
            changes.push(json!({"path": path, "note": "entered the contract's inputs"}));
            continue;
        };
        if previous.after != file.after {
            let before = match previous.after.as_deref() {
                Some(hash) => {
                    if let Some(text) = review.read_blob(hash)? {
                        text
                    } else {
                        changes.push(json!({"path": path, "note": forgotten}));
                        continue;
                    }
                },
                None => String::new(),
            };
            let after = entry
                .changes
                .iter()
                .find(|change| change.path == file.path)
                .and_then(|change| change.after.clone())
                .unwrap_or_default();
            let lines = TextDiff::from_lines(&before, &after);
            let count = |tag| {
                lines
                    .iter_all_changes()
                    .filter(|change| change.tag() == tag)
                    .count()
            };
            let (added, removed) = (count(ChangeTag::Insert), count(ChangeTag::Delete));
            let text = diff(&path, old.rev, &before, &after);
            changes.push(json!({"path": path, "diff": text, "added": added, "removed": removed}));
        } else if previous.before != file.before {
            changes.push(json!({"path": path, "note": "base contents changed"}));
        }
    }
    let current_paths: HashSet<_> = entry.files.iter().map(|scoped| &scoped.file.path).collect();
    for (_, file) in revision.list_files(old) {
        if !current_paths.contains(&file.path) {
            let path = file.path.display().to_string();
            changes.push(json!({"path": path, "note": "left the contract's inputs"}));
        }
    }
    Ok(changes)
}

fn diff(path: &str, rev: u64, before: &str, after: &str) -> String {
    TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(3)
        .header(&format!("{path} (r{rev})"), &format!("{path} (now)"))
        .to_string()
}

fn format_span_text(span: &review::Span) -> String {
    match span.lines {
        Some([start, end]) if start == end => format!("{}:{start}", span.path.display()),
        Some([start, end]) => format!("{}:{start}-{end}", span.path.display()),
        None => span.path.display().to_string(),
    }
}

/// The check that captures this selection again against the review's base commit, naming
/// `contract` if given.
#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
fn check_command(revision: &Revision, contract: Option<&str>) -> String {
    let mut command = String::from("tenet check");
    for path in &revision.paths {
        command.push(' ');
        command.push_str(&path.display().to_string());
    }
    if let Some(base) = &revision.base_oid {
        command.push_str(" --base ");
        command.push_str(base);
    }
    if let Some(contract) = contract {
        command.push_str(" --contract ");
        command.push_str(contract);
    }
    for request in &revision.context {
        command.push_str(" --context ");
        command.push_str(&request.to_string());
    }
    command
}

#[expect(
    clippy::integer_division,
    reason = "The division rounds down by design: whole spinner frames and whole minutes, hours \
              and days."
)]
#[expect(
    clippy::integer_division_remainder_used,
    reason = "The quotient and remainder pick a spinner frame or a whole time unit, and \
              tokio::select! expands to a remainder for its branch order."
)]
fn format_ago(at: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(at, |duration| duration.as_secs());
    match now.saturating_sub(at) {
        0..60 => "just now".into(),
        seconds @ 60..3600 => format!("{}m ago", seconds / 60),
        seconds @ 3600..86400 => format!("{}h ago", seconds / 3600),
        seconds => format!("{}d ago", seconds / 86400),
    }
}
