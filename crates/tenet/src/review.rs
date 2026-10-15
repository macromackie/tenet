//! Commands that read and extend the review kept in `.tenet/`: status, show, record, and log.

use std::{
    collections::HashSet,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde_json::{Value, json};
use similar::TextDiff;
use tenet_engine::{
    ContractState, ContractStatus, Pending, Review,
    review::{
        self, Assessment, CurrentContract, Entry, Record, RecordRequest, Revision, SpanRequest,
    },
};

use crate::report_format::{marker, paint};

pub(crate) async fn status(root: &Path, json: bool) -> Result<u8> {
    let review = Review::existing(root)?;
    let current = review::current(root, &review, None).await?;
    let code = review::exit_code(current.contracts.iter().map(|c| &c.state));
    let mut out = io::stdout().lock();
    if json {
        let contracts = current
            .contracts
            .iter()
            .map(|entry| {
                let mut value = serde_json::to_value(&entry.state)?;
                value["contract"] = json!(entry.contract.name);
                value["path"] = json!(entry.contract.path);
                value["input"] = json!(entry.input);
                value["assessed_in"] = json!(entry.assessment.as_ref().map(|a| a.rev));
                value["model"] = json!(model_conclusion(entry.assessment.as_ref()));
                Ok(value)
            })
            .collect::<Result<Vec<_>>>()?;
        writeln!(
            out,
            "{}",
            json!({"version": 1, "review": current.latest.review, "revision": current.latest.rev, "contracts": contracts, "exit_code": code})
        )?;
        return Ok(code);
    }
    writeln!(
        out,
        "Review {} · latest check r{} {}\n",
        current.latest.review,
        current.latest.rev,
        ago(current.latest.at)
    )?;
    let width = current
        .contracts
        .iter()
        .map(|c| c.contract.name.len())
        .max()
        .unwrap_or(0);
    for entry in &current.contracts {
        let (label, note) = state_text(entry, current.latest.rev);
        writeln!(
            out,
            " {} {:width$}  {:10}  {note}",
            marker(label),
            entry.contract.name,
            paint(&label.to_lowercase()),
        )?;
    }
    let pending: Vec<_> = current
        .contracts
        .iter()
        .filter(|c| c.state.is_pending())
        .collect();
    writeln!(
        out,
        "\n {} settled · {} pending",
        current.contracts.len() - pending.len(),
        pending.len()
    )?;
    let stale = pending.iter().any(|c| {
        matches!(
            c.state,
            ContractState::Pending {
                reason: Pending::InputsChanged | Pending::NotChecked
            }
        )
    });
    if stale {
        writeln!(out, " Next  {}", check_command(&current.latest))?;
    } else if let Some(first) = pending.first() {
        writeln!(out, " Next  tenet show {}", first.contract.name)?;
    }
    Ok(code)
}

pub(crate) async fn show(root: &Path, name: &str, json: bool) -> Result<u8> {
    let review = Review::existing(root)?;
    let current = review::current(root, &review, Some(name)).await?;
    let entry = current
        .contracts
        .first()
        .with_context(|| format!("{name} is not part of the current review; see tenet status"))?;
    let records: Vec<&Record> = review
        .records()
        .rev()
        .filter(|r| r.contract == name)
        .collect();
    let mut inputs = HashSet::from([entry.input.as_str()]);
    let earlier: Vec<&Assessment> = review
        .assessments()
        .rev()
        .filter(|a| a.contract == name && inputs.insert(a.input.as_str()))
        .collect();
    let changes_since = earlier
        .first()
        .map(|old| interdiff(&review, old, entry))
        .transpose()?
        .unwrap_or_default();
    let conclusions = review::conclusions(entry.mode, entry.scope);
    let mut out = io::stdout().lock();
    if json {
        writeln!(
            out,
            "{}",
            json!({
                "version": 1,
                "current": entry,
                "document": entry.contract.document,
                "records": records,
                "earlier": earlier,
                "changes_since_earlier": changes_since,
                "conclusions": conclusions,
            })
        )?;
        return Ok(0);
    }

    let (label, note) = state_text(entry, current.latest.rev);
    writeln!(
        out,
        "{}  {}  {note}",
        entry.contract.name,
        label.to_lowercase()
    )?;
    writeln!(out, "  {}", entry.contract.path.display())?;
    if let Some(assessment) = &entry.assessment {
        writeln!(
            out,
            "  Assessed in r{} · {} · input {}",
            assessment.rev,
            scope_text(entry),
            &entry.input[..12]
        )?;
    }

    if let Some(assessment) = &entry.assessment {
        writeln!(out, "\nModel")?;
        match &assessment.conclusion {
            Some(conclusion) => {
                writeln!(
                    out,
                    "  {}{}: {}",
                    conclusion.status.as_str(),
                    percent(conclusion.confidence),
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
        for result in &assessment.results {
            writeln!(
                out,
                "  {}  {}{}",
                result.path,
                crate::report_format::status(result.status).to_lowercase(),
                percent(result.confidence)
            )?;
        }
    }

    writeln!(out, "\nFiles")?;
    for file in &entry.files {
        let kind = match (file.role, &file.before, &file.after) {
            (review::Role::Support, ..) => "support",
            _ if file.error.is_some() => "unreadable",
            _ if entry.mode == tenet_engine::Mode::Full => "selected",
            (_, None, _) => "added",
            (_, _, None) => "deleted",
            _ if file.previous_path.is_some() => "renamed",
            _ => "modified",
        };
        writeln!(out, "  {kind:10} {}", file.path.display())?;
        if let Some(error) = &file.error {
            writeln!(out, "             {error}")?;
        }
    }

    if !records.is_empty() || !earlier.is_empty() {
        writeln!(out, "\nHistory")?;
        for record in &records {
            let currency = if record.input == entry.input {
                "current"
            } else {
                "outdated"
            };
            let spans: Vec<_> = record.evidence.iter().map(span_text).collect();
            writeln!(
                out,
                "  record {}  {}  {}  {currency}\n    {}\n    {}",
                record.id,
                record.conclusion.as_str(),
                ago(record.at),
                record.message,
                spans.join(", ")
            )?;
        }
        for assessment in earlier.iter().take(3) {
            writeln!(
                out,
                "  r{}  model {}  outdated",
                assessment.rev,
                model_conclusion(Some(assessment)).unwrap_or("not assessed")
            )?;
        }
        if let Some(old) = earlier.first() {
            writeln!(out, "\nChanges since r{}", old.rev)?;
            for change in &changes_since {
                match (&change["diff"], &change["note"]) {
                    (Value::String(diff), _) => writeln!(out, "{}", diff.trim_end())?,
                    (_, note) => writeln!(
                        out,
                        "  {}: {}",
                        change["path"].as_str().unwrap_or(""),
                        note.as_str().unwrap_or("")
                    )?,
                }
            }
        }
    }

    writeln!(out, "\nRecord")?;
    let choices: Vec<_> = conclusions.iter().map(|c| c.as_str()).collect();
    writeln!(
        out,
        "  tenet record {name} --conclusion {} --evidence PATH:START-END -m \"what the evidence shows\"",
        choices.join("|")
    )?;

    writeln!(out, "\nContract\n\n{}", entry.contract.document.trim_end())?;
    let patches: Vec<_> = entry
        .changes
        .iter()
        .filter(|c| !c.patch.is_empty())
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
    Ok(0)
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
    let record = review::record(
        root,
        &RecordRequest {
            contract: args.contract.clone(),
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
    let mut out = io::stdout().lock();
    if json {
        let revisions: Vec<u64> = review
            .assessments()
            .filter(|a| contract.is_none_or(|name| name == a.contract))
            .map(|a| a.rev)
            .collect();
        for entry in review.entries() {
            let shown = match entry {
                Entry::Revision(revision) => revisions.contains(&revision.rev),
                Entry::Assessment(assessment) => {
                    contract.is_none_or(|name| name == assessment.contract)
                }
                Entry::Record(record) => contract.is_none_or(|name| name == record.contract),
            };
            if shown {
                let mut value = serde_json::to_value(entry)?;
                value["version"] = json!(1);
                writeln!(out, "{value}")?;
            }
        }
        return Ok(0);
    }
    let mut shown = false;
    for revision in review.revisions().rev() {
        let assessments: Vec<_> = review
            .assessments()
            .filter(|a| a.rev == revision.rev && contract.is_none_or(|name| name == a.contract))
            .collect();
        if assessments.is_empty() {
            continue;
        }
        shown = true;
        writeln!(
            out,
            "r{}  {}  {}  {}",
            revision.rev,
            ago(revision.at),
            revision.review,
            revision.model.as_deref().unwrap_or("no model")
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
                model_conclusion(Some(assessment)).unwrap_or("not assessed"),
                &assessment.input[..12]
            )?;
            for record in review.records().filter(|r| {
                r.contract == assessment.contract
                    && r.input == assessment.input
                    && r.rev == revision.rev
            }) {
                writeln!(
                    out,
                    "    record {}  {}  {}  {}",
                    record.id,
                    record.conclusion.as_str(),
                    ago(record.at),
                    record.message
                )?;
            }
        }
    }
    if !shown {
        writeln!(out, "No checks recorded.")?;
    }
    Ok(0)
}

fn state_text(entry: &CurrentContract, latest: u64) -> (&'static str, String) {
    match &entry.state {
        ContractState::Settled {
            conclusion,
            record: Some(id),
        } => (upper(*conclusion), format!("recorded {id}")),
        ContractState::Settled {
            conclusion,
            record: None,
        } => (
            upper(*conclusion),
            format!(
                "model{}",
                percent(
                    entry
                        .assessment
                        .as_ref()
                        .and_then(|a| a.conclusion.as_ref())
                        .and_then(|c| c.confidence)
                )
            ),
        ),
        ContractState::Pending { reason } => {
            let confidence = percent(
                entry
                    .assessment
                    .as_ref()
                    .and_then(|a| a.conclusion.as_ref())
                    .and_then(|c| c.confidence),
            );
            let note = match reason {
                Pending::ModelFailed => format!("model failed{confidence} · confirm or dispute"),
                Pending::ModelUnresolved => format!("model unresolved{confidence}"),
                Pending::NotAssessed => "not assessed".into(),
                Pending::InputsChanged => format!(
                    "inputs changed after r{}",
                    entry.latest.as_ref().map_or(latest, |a| a.rev)
                ),
                Pending::NotChecked => "not checked".into(),
            };
            ("PENDING", note)
        }
    }
}

fn upper(status: ContractStatus) -> &'static str {
    match status {
        ContractStatus::Verified => "VERIFIED",
        ContractStatus::Preserved => "PRESERVED",
        ContractStatus::Clear => "CLEAR",
        ContractStatus::Unaffected => "UNAFFECTED",
        ContractStatus::Failed => "FAILED",
        ContractStatus::Unresolved => "UNRESOLVED",
    }
}

fn scope_text(entry: &CurrentContract) -> &'static str {
    match (entry.mode, entry.scope) {
        (tenet_engine::Mode::Diff, tenet_engine::RequestedScope::FullContract) => {
            "diff check of the full contract"
        }
        (tenet_engine::Mode::Diff, _) => "diff check of selected subjects",
        (_, tenet_engine::RequestedScope::FullContract) => "full check of the full contract",
        _ => "full check of selected subjects",
    }
}

fn model_conclusion(assessment: Option<&Assessment>) -> Option<&'static str> {
    assessment
        .and_then(|a| a.conclusion.as_ref())
        .map(|c| c.status.as_str())
}

/// What changed in the contract's inputs between an earlier assessment and now.
fn interdiff(review: &Review, old: &Assessment, entry: &CurrentContract) -> Result<Vec<Value>> {
    let mut changes = Vec::new();
    let path = entry.contract.path.display().to_string();
    let forgotten = format!("contents from r{} are no longer in .tenet/blobs", old.rev);
    if old.contract_hash != entry.contract.hash {
        match review.blob(&old.contract_hash)? {
            Some(before) => {
                let text = diff(&path, old.rev, &before, &entry.contract.document);
                changes.push(json!({"path": path, "diff": text}));
            }
            None => changes.push(json!({"path": path, "note": forgotten})),
        }
    }
    for file in &entry.files {
        let path = file.path.display().to_string();
        let Some(previous) = old.files.iter().find(|f| f.path == file.path) else {
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
            let text = diff(&path, old.rev, &before, &after);
            changes.push(json!({"path": path, "diff": text}));
        } else if previous.before != file.before {
            changes.push(json!({"path": path, "note": "base contents changed"}));
        }
    }
    for previous in &old.files {
        if !entry.files.iter().any(|f| f.path == previous.path) {
            let path = previous.path.display().to_string();
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

fn percent(confidence: Option<f64>) -> String {
    confidence
        .map(|score| format!(" ({:.0}%)", score * 100.0))
        .unwrap_or_default()
}

fn check_command(revision: &Revision) -> String {
    let mut command = String::from("tenet check");
    for path in &revision.paths {
        command.push(' ');
        command.push_str(&path.display().to_string());
    }
    if let Some(base) = &revision.base {
        command.push_str(" --base ");
        command.push_str(base);
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
