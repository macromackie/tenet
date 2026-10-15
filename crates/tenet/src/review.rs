//! Commands that read and extend the review kept in `.tenet/`: status, show, record, and log.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde_json::{Value, json};
use similar::{ChangeTag, TextDiff};
use tenet_engine::{
    Change, ContractState, ContractStatus, MIN_CONFIDENCE, Mode, Pending, Review, Status,
    review::{
        self, Assessment, Current, CurrentContract, Entry, MAX_BLOB_BYTES, Record, RecordRequest,
        Recorded, Revision, Role, SpanRequest, Unchecked,
    },
};

use crate::report_format::{Model, marker, paint, violation};

/// How many records and earlier assessments `show` lists; `log NAME` lists every one.
const HISTORY: usize = 5;
/// `show` lists every file and file result of a scope this size or smaller, and otherwise only
/// those that need attention.
const LISTED: usize = 20;
/// How many unreadable files `show` names; it counts the rest.
const UNREADABLE: usize = 5;
/// How many changed ranges `show` offers as evidence.
const CITATIONS: usize = 10;

pub(crate) async fn status(root: &Path, base: Option<&str>, json: bool) -> Result<u8> {
    let review = Review::existing(root)?;
    let mut out = io::stdout().lock();
    let current = match review::current(root, &review, base, None).await {
        Ok(current) => current,
        Err(error) => {
            let Some(unchecked) = error.downcast_ref::<Unchecked>() else {
                return Err(error);
            };
            // The review against this commit has not begun, so nothing in it is settled.
            if json {
                let message = unchecked.to_string();
                let base = &unchecked.base;
                writeln!(
                    out,
                    "{}",
                    json!({"version": 1, "base": base, "revision": null, "contracts": [], "exit_code": 4, "message": message})
                )?;
            } else {
                writeln!(out, "{unchecked}")?;
            }
            return Ok(4);
        }
    };
    let code = review::exit_code(current.contracts.iter().map(|c| &c.state));
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
                value["assessed_in"] = json!(entry.assessment.map(|a| a.rev));
                value["model"] = json!(model(entry).map(|m| m.status));
                value["model_violation"] = json!(model(entry).and_then(|m| m.violation));
                value["model_answer"] = json!(model(entry).and_then(|m| m.answer));
                value["min_confidence"] = json!(entry.min_confidence);
                Ok(value)
            })
            .collect::<Result<Vec<_>>>()?;
        writeln!(
            out,
            "{}",
            json!({
                "version": 1,
                "review": current.latest.review,
                "base": current.latest.base_oid,
                "revision": current.latest.rev,
                "unchanged": current.unchanged,
                "contracts": contracts,
                "deleted_contracts": current.deleted,
                "exit_code": code,
            })
        )?;
        return Ok(code);
    }
    writeln!(
        out,
        "Review {}{} · latest check r{} {}\n",
        current.latest.review,
        paths_note(current.latest),
        current.latest.rev,
        ago(current.latest.at)
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
        .map(|c| c.contract.name.len())
        .max()
        .unwrap_or(0);
    for entry in &current.contracts {
        let (label, mut note) = state_text(entry);
        if !entry.selection.selects(&current.latest.paths) {
            note.push_str(&format!(
                " · from r{} {}",
                entry.selection.rev,
                selection_text(entry.selection)
            ));
        }
        writeln!(
            out,
            " {} {:width$}  {:10}  {note}",
            marker(label),
            entry.contract.name,
            paint(&label.to_lowercase()),
        )?;
    }
    for path in &current.deleted {
        writeln!(
            out,
            " {} {}  deleted by this change; its requirement no longer applies",
            marker("DELETED"),
            path.display()
        )?;
    }
    let pending: Vec<_> = current
        .contracts
        .iter()
        .filter(|c| c.state.is_pending())
        .collect();
    let recorded = current
        .contracts
        .iter()
        .filter(|c| {
            matches!(
                c.state,
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
    let stale = pending.iter().find(|c| {
        matches!(
            c.state,
            ContractState::Pending {
                reason: Pending::InputsChanged | Pending::NotChecked | Pending::NotAssessed
            }
        )
    });
    let violation = |c: &CurrentContract| model(c).and_then(|m| m.violation).unwrap_or(-1.0);
    if let Some(stale) = stale {
        // A contract no check assessed needs a check of every contract, not of the one named.
        let contract = match stale.state {
            ContractState::Pending {
                reason: Pending::NotChecked,
            } => None,
            _ => stale.selection.contract.as_deref(),
        };
        writeln!(out, " Next  {}", check_command(stale.selection, contract))?;
    } else if let Some(first) = pending
        .iter()
        .max_by(|a, b| violation(a).total_cmp(&violation(b)))
    {
        writeln!(out, " Next  tenet show {}", row_name(&current, first))?;
    }
    Ok(code)
}

pub(crate) async fn show(root: &Path, name: &str, paths: &[PathBuf], json: bool) -> Result<u8> {
    let review = Review::existing(root)?;
    let current = review::current(root, &review, None, Some(name)).await?;
    let entry = current.select(name, paths)?;
    let mut out = BufWriter::new(io::stdout().lock());
    if !json {
        brief(&mut out, &review, &current, entry)?;
        change(&mut out, &entry.changes)?;
        out.flush()?;
        return Ok(0);
    }

    let history = History::new(&review, entry)?;
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
        .collect::<Result<_>>()?;
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
    writeln!(
        out,
        "{}",
        json!({
            "version": 2,
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
            "earlier": earlier.iter().take(HISTORY).map(|a| shown(a)).collect::<Vec<_>>(),
            "older": {
                "records": records.len().saturating_sub(HISTORY),
                "assessments": earlier.len().saturating_sub(HISTORY),
            },
            "changes_since_earlier": history.changes_since,
            "conclusions": review::conclusions(entry.mode, entry.scope),
        })
    )?;
    out.flush()?;
    Ok(0)
}

/// Every pending contract's brief, then the change they share, printed once.
pub(crate) async fn show_pending(root: &Path) -> Result<u8> {
    let review = Review::existing(root)?;
    let current = review::current(root, &review, None, None).await?;
    let pending: Vec<_> = (current.contracts.iter())
        .filter(|entry| entry.state.is_pending())
        .collect();
    let mut out = BufWriter::new(io::stdout().lock());
    if pending.is_empty() {
        writeln!(out, "No contract is pending.")?;
    }
    for (index, entry) in pending.iter().enumerate() {
        if index > 0 {
            writeln!(out, "\n{}\n", "─".repeat(72))?;
        }
        brief(&mut out, &review, &current, entry)?;
    }
    let mut seen = HashSet::new();
    let changes: Vec<_> = (pending.iter())
        .flat_map(|entry| &entry.changes)
        .filter(|change| seen.insert(&change.path))
        .cloned()
        .collect();
    change(&mut out, &changes)?;
    out.flush()?;
    Ok(0)
}

/// A contract's records and earlier assessments, newest first, and what changed in its inputs
/// since the latest earlier assessment.
struct History<'a> {
    records: Vec<&'a Record>,
    earlier: Vec<&'a Assessment>,
    changes_since: Vec<Value>,
}

impl<'a> History<'a> {
    fn new(review: &'a Review, entry: &CurrentContract) -> Result<Self> {
        let name = &entry.contract.name;
        let records = (review.records().rev())
            .filter(|r| &r.contract == name)
            .collect();
        let mut inputs = HashSet::from([entry.input.as_str()]);
        let earlier: Vec<_> = (review.assessments().rev())
            .filter(|a| &a.contract == name && inputs.insert(a.input.as_str()))
            .collect();
        let changes_since = earlier
            .first()
            .map(|old| interdiff(review, old, entry))
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
fn brief(
    out: &mut impl Write,
    review: &Review,
    current: &Current,
    entry: &CurrentContract,
) -> Result<()> {
    let name = &entry.contract.name;
    let History {
        records,
        earlier,
        changes_since,
    } = History::new(review, entry)?;
    let (label, note) = state_text(entry);
    writeln!(out, "{name}  {}  {note}", label.to_lowercase())?;
    writeln!(out, "  {}", entry.contract.path.display())?;
    writeln!(
        out,
        "  Review {}{} · selection checked in r{}",
        entry.selection.review,
        paths_note(entry.selection),
        entry.selection.rev
    )?;
    if let Some(assessment) = entry.assessment {
        writeln!(
            out,
            "  Assessed in r{} · {} check of {} · input {}{}",
            assessment.rev,
            entry.mode.as_str(),
            entry.scope.description(),
            &entry.input[..12],
            threshold_note(entry.min_confidence)
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
            "\n  This change adds or edits the contract, so the diff check's assumption that the base\n  \
             complied with it does not hold. Assess the whole contract in a separate full review\n  \
             with `tenet check --contract {name}`, then return with `{}`\n  \
             and record a conclusion here.",
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
                    violation(conclusion.violation),
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
            }
            None => writeln!(out, "  not assessed")?,
        }
        let results = &assessment.results;
        if results.len() > LISTED {
            let statuses = results
                .iter()
                .map(|result| crate::report_format::status(result.status));
            writeln!(out, "  File checks: {}", tally(statuses))?;
        }
        let attention = |status: Status| !matches!(status, Status::Pass | Status::NotApplicable);
        for result in results
            .iter()
            .filter(|result| results.len() <= LISTED || attention(result.status))
        {
            let omitted = crate::report_format::omitted(&result.omitted);
            writeln!(
                out,
                "  {}  {}{}{}",
                result.path,
                crate::report_format::status(result.status).to_lowercase(),
                violation(result.violation),
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
        tally(kinds.iter().map(|(kind, _)| *kind))
    )?;
    let listed = kinds
        .iter()
        .filter(|(kind, _)| kinds.len() <= LISTED || !matches!(*kind, "selected" | "modified"));
    let mut unreadable = 0;
    for (kind, scoped) in listed {
        if *kind == "unreadable" {
            unreadable += 1;
            if unreadable > UNREADABLE {
                continue;
            }
        }
        writeln!(out, "  {kind:10} {}", scoped.file.path.display())?;
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
            row_name(current, entry)
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
                ago(record.at),
                record.message.replace('\n', "\n    "),
            )?;
            if !record.evidence.is_empty() {
                let spans: Vec<_> = record.evidence.iter().map(span_text).collect();
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
                    .map_or("not assessed", |c| c.status.as_str())
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
                        )?
                    }
                    (Value::String(diff), _) => writeln!(out, "{}", diff.trim_end())?,
                    (_, note) => writeln!(out, "  {path}: {}", note.as_str().unwrap_or(""))?,
                }
            }
        }
    }

    writeln!(out, "\nRecord")?;
    let conclusions = review::conclusions(entry.mode, entry.scope);
    let choices: Vec<_> = conclusions.iter().map(|c| c.as_str()).collect();
    writeln!(
        out,
        "  tenet record {} --conclusion {} --evidence PATH:START-END -m \"what the evidence shows\"",
        row_name(current, entry),
        choices.join("|")
    )?;
    let flagged: Vec<_> = entry
        .assessment
        .iter()
        .flat_map(|a| &a.results)
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
            "  A favorable conclusion needs `tenet check --contract {name}` settled as verified first"
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
fn change(out: &mut impl Write, changes: &[Change]) -> Result<()> {
    let patches: Vec<_> = changes.iter().filter(|c| !c.patch.is_empty()).collect();
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

pub(crate) async fn record(root: &Path, args: &crate::cli::Record) -> Result<u8> {
    let conclusion = ContractStatus::parse(&args.conclusion).with_context(|| {
        format!(
            "unknown conclusion {}; use verified, preserved, unaffected, clear, failed, or unresolved",
            args.conclusion
        )
    })?;
    let evidence = args
        .evidence
        .iter()
        .map(|text| span(text))
        .collect::<Result<Vec<_>>>()?;
    let Recorded { record, replaces } = review::record(
        root,
        &RecordRequest {
            contract: args.contract.clone(),
            paths: args.paths.clone(),
            conclusion,
            evidence,
            message: args.message.clone(),
        },
    )
    .await?;
    let mut out = io::stdout().lock();
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
            "The model concluded {}; this record decides the contract while its input is unchanged.",
            model.as_str()
        )?;
    }
    Ok(0)
}

pub(crate) fn log(root: &Path, contract: Option<&str>, json: bool) -> Result<u8> {
    let review = Review::existing(root)?;
    let shown = |name: &str| contract.is_none_or(|contract| contract == name);
    let mut out = BufWriter::new(io::stdout().lock());
    if json {
        for entry in review.entries() {
            let listed = match entry {
                Entry::Revision(revision) => review
                    .assessments_in(revision.rev)
                    .any(|a| shown(&a.contract)),
                Entry::Assessment(assessment) => shown(&assessment.contract),
                Entry::Record(record) => shown(&record.contract),
            };
            if listed {
                let mut value = serde_json::to_value(entry)?;
                value["version"] = json!(2);
                writeln!(out, "{value}")?;
            }
        }
        out.flush()?;
        return Ok(0);
    }

    // A record names the one assessment its revision made of its contract.
    let mut records: HashMap<(u64, &str), Vec<&Record>> = HashMap::new();
    for record in review.records() {
        records
            .entry((record.rev, &record.contract))
            .or_default()
            .push(record);
    }
    let mut listed = false;
    for revision in review.revisions().rev() {
        let mut assessments = review
            .assessments_in(revision.rev)
            .filter(|a| shown(&a.contract))
            .peekable();
        if assessments.peek().is_none() {
            continue;
        }
        listed = true;
        writeln!(
            out,
            "r{}  {}  {}{}  {}{}",
            revision.rev,
            ago(revision.at),
            revision.review,
            paths_note(revision),
            revision.model.as_deref().unwrap_or("no model"),
            threshold_note(revision.min_confidence)
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
                    .map_or("not assessed", |c| c.status.as_str()),
                &assessment.input[..12]
            )?;
            let made = records.get(&(revision.rev, assessment.contract.as_str()));
            for record in made.into_iter().flatten() {
                writeln!(
                    out,
                    "    record {}  {}  {}  {}",
                    record.id,
                    record.conclusion.as_str(),
                    ago(record.at),
                    record.message.replace('\n', "\n      ")
                )?;
            }
        }
    }
    if !listed {
        writeln!(out, "No checks recorded.")?;
    }
    out.flush()?;
    Ok(0)
}

/// The model's conclusion for the row's exact input.
fn model(entry: &CurrentContract) -> Option<Model> {
    let conclusion = entry.assessment?.conclusion.as_ref()?;
    Some(Model {
        status: conclusion.status,
        violation: conclusion.violation,
        answer: conclusion.answer,
        min_confidence: entry.min_confidence,
    })
}

/// The row's state as `check` reports it, and which check the changed inputs were captured in.
fn state_text(entry: &CurrentContract) -> (&'static str, String) {
    let threshold = threshold_note(entry.min_confidence);
    let (label, mut note) = crate::report_format::state(&entry.state, model(entry), &threshold);
    if let ContractState::Pending {
        reason: Pending::InputsChanged,
    } = entry.state
    {
        note.push_str(&format!(" after r{}", entry.selection.rev));
    }
    (label, note)
}

/// How many of each label, in label order: "2 fail · 30 pass".
fn tally<'a>(labels: impl Iterator<Item = &'a str>) -> String {
    let mut counts = BTreeMap::new();
    for label in labels {
        *counts.entry(label.to_lowercase()).or_insert(0) += 1;
    }
    let counts: Vec<_> = counts
        .iter()
        .map(|(label, count)| format!("{count} {label}"))
        .collect();
    counts.join(" · ")
}

/// The name that selects this row in `show` and `record`: the contract, plus the selection's
/// paths when the contract stands in more than one check.
fn row_name(current: &Current, entry: &CurrentContract) -> String {
    let mut name = entry.contract.name.clone();
    let rows = current
        .contracts
        .iter()
        .filter(|c| c.contract.name == name)
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
fn paths_note(revision: &Revision) -> String {
    if revision.paths.is_empty() {
        String::new()
    } else {
        format!(" · {}", selection_text(revision))
    }
}

fn selection_text(revision: &Revision) -> String {
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
fn threshold_note(min_confidence: Option<f64>) -> String {
    match min_confidence {
        Some(threshold) if threshold != MIN_CONFIDENCE => {
            format!(" · min confidence {threshold}")
        }
        _ => String::new(),
    }
}

/// What changed in the contract's inputs between an earlier assessment and now.
fn interdiff(review: &Review, old: &Assessment, entry: &CurrentContract) -> Result<Vec<Value>> {
    let mut changes = Vec::new();
    let path = entry.contract.path.display().to_string();
    let forgotten = format!(
        "contents from r{} are not in .tenet/blobs, which keeps files up to {} MiB",
        old.rev,
        MAX_BLOB_BYTES >> 20
    );
    if old.contract_hash != entry.contract.hash {
        match review.blob(&old.contract_hash)? {
            Some(before) => {
                let text = diff(&path, old.rev, &before, &entry.contract.document);
                changes.push(json!({"path": path, "diff": text}));
            }
            None => changes.push(json!({"path": path, "note": forgotten})),
        }
    }
    let revision = review.revision(old.rev)?;
    let previous: HashMap<_, _> = revision
        .files(old)
        .map(|(_, file)| (&file.path, file))
        .collect();
    for scoped in &entry.files {
        let file = &scoped.file;
        let path = file.path.display().to_string();
        let Some(previous) = previous.get(&file.path) else {
            changes.push(json!({"path": path, "note": "entered the contract's inputs"}));
            continue;
        };
        if previous.after != file.after {
            let before = match previous.after.as_deref() {
                Some(hash) => match review.blob(hash)? {
                    Some(text) => text,
                    None => {
                        changes.push(json!({"path": path, "note": forgotten}));
                        continue;
                    }
                },
                None => String::new(),
            };
            let after = entry
                .changes
                .iter()
                .find(|c| c.path == file.path)
                .and_then(|c| c.after.clone())
                .unwrap_or_default();
            let lines = TextDiff::from_lines(&before, &after);
            let count = |tag| lines.iter_all_changes().filter(|c| c.tag() == tag).count();
            let (added, removed) = (count(ChangeTag::Insert), count(ChangeTag::Delete));
            let text = diff(&path, old.rev, &before, &after);
            changes.push(json!({"path": path, "diff": text, "added": added, "removed": removed}));
        } else if previous.before != file.before {
            changes.push(json!({"path": path, "note": "base contents changed"}));
        }
    }
    let current: HashSet<_> = entry.files.iter().map(|scoped| &scoped.file.path).collect();
    for (_, file) in revision.files(old) {
        if !current.contains(&file.path) {
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

fn span(text: &str) -> Result<SpanRequest> {
    if let Some((path, lines)) = text.rsplit_once(':')
        && !lines.is_empty()
        && lines.chars().all(|c| c.is_ascii_digit() || c == '-')
    {
        let (start, end) = lines.split_once('-').unwrap_or((lines, lines));
        let start: usize = start
            .parse()
            .with_context(|| format!("invalid line range in {text}"))?;
        let end: usize = end
            .parse()
            .with_context(|| format!("invalid line range in {text}"))?;
        return Ok(SpanRequest {
            path: PathBuf::from(path),
            lines: Some((start, end)),
        });
    }
    Ok(SpanRequest {
        path: PathBuf::from(text),
        lines: None,
    })
}

fn span_text(span: &review::Span) -> String {
    match span.lines {
        Some([start, end]) if start == end => format!("{}:{start}", span.path.display()),
        Some([start, end]) => format!("{}:{start}-{end}", span.path.display()),
        None => span.path.display().to_string(),
    }
}

/// The check that captures this selection again against the review's base commit, naming
/// `contract` if given.
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
    for path in &revision.context {
        command.push_str(" --context ");
        command.push_str(&path.display().to_string());
    }
    command
}

fn ago(at: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(at, |d| d.as_secs());
    match now.saturating_sub(at) {
        0..60 => "just now".into(),
        seconds @ 60..3600 => format!("{}m ago", seconds / 60),
        seconds @ 3600..86400 => format!("{}h ago", seconds / 3600),
        seconds => format!("{}d ago", seconds / 86400),
    }
}
