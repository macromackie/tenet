//! An agent's conclusion for a contract's current input, validated before it is appended.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};

use super::{Entry, Record, Review, Span, current, now};
use crate::{ContractState, ContractStatus, Mode, Pending, RequestedScope};

pub struct RecordRequest {
    pub contract: String,
    pub conclusion: ContractStatus,
    pub evidence: Vec<SpanRequest>,
    pub message: String,
}

pub struct SpanRequest {
    pub path: PathBuf,
    /// Inclusive, 1-based; absent to cite the whole file.
    pub lines: Option<(usize, usize)>,
}

/// Conclusions a record may state, which are those a check of the same mode and scope can reach.
pub fn conclusions(mode: Mode, scope: RequestedScope) -> &'static [ContractStatus] {
    use ContractStatus::*;
    match (mode, scope) {
        (_, RequestedScope::SelectedSubjects) => &[Clear, Failed, Unresolved],
        (Mode::Full, RequestedScope::FullContract) => &[Verified, Failed, Unresolved],
        (Mode::Diff, RequestedScope::FullContract) => &[Preserved, Unaffected, Failed, Unresolved],
    }
}

/// Bind a conclusion to the input the latest check assessed. Refuse it when the contract's scoped
/// files changed since, so a verdict never attaches to code its author did not read.
pub async fn record(root: &Path, request: &RecordRequest) -> Result<Record> {
    let review = Review::existing(root)?;
    let current = current(root, &review, Some(&request.contract)).await?;
    let entry = current.contracts.into_iter().next().with_context(|| {
        format!(
            "{} is not part of the current review; see tenet status",
            request.contract
        )
    })?;
    let Some(assessment) = &entry.assessment else {
        match entry.state {
            ContractState::Pending {
                reason: Pending::NotChecked,
            } => bail!(
                "{} has not been checked; run tenet check first",
                request.contract
            ),
            _ => bail!(
                "{}: scoped files changed after r{}; run tenet check before recording",
                request.contract,
                entry.latest.as_ref().map_or(current.latest.rev, |a| a.rev)
            ),
        }
    };
    let allowed = conclusions(entry.mode, entry.scope);
    ensure!(
        allowed.contains(&request.conclusion),
        "{} does not fit a {} check of {}; record one of: {}",
        request.conclusion.as_str(),
        mode_name(entry.mode),
        scope_name(entry.scope),
        allowed
            .iter()
            .map(|c| c.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    ensure!(
        !request.message.trim().is_empty(),
        "state what the evidence shows with --message"
    );
    ensure!(
        request.conclusion == ContractStatus::Unresolved || !request.evidence.is_empty(),
        "cite the code that supports {} with --evidence PATH:START-END",
        request.conclusion.as_str()
    );

    let mut evidence = Vec::new();
    for span in &request.evidence {
        ensure!(
            span.path
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
            "evidence paths must be relative to the project root: {}",
            span.path.display()
        );
        let full = root.join(&span.path);
        if !full.exists()
            && let Some(file) = entry
                .files
                .iter()
                .find(|f| f.path == span.path && f.after.is_none())
        {
            ensure!(
                span.lines.is_none(),
                "{} was deleted; cite it without lines",
                span.path.display()
            );
            evidence.push(Span {
                path: span.path.clone(),
                lines: None,
                blob: file
                    .before
                    .clone()
                    .with_context(|| format!("{} has no captured contents", span.path.display()))?,
            });
            continue;
        }
        let text = fs::read_to_string(&full)
            .with_context(|| format!("cannot read evidence {}", span.path.display()))?;
        let count = text.lines().count();
        if let Some((start, end)) = span.lines {
            ensure!(
                (1..=end).contains(&start) && end <= count,
                "{}:{start}-{end} is outside the file's {count} lines",
                span.path.display()
            );
        }
        evidence.push(Span {
            path: span.path.clone(),
            lines: span.lines.map(|(start, end)| [start, end]),
            blob: review.keep(&text)?,
        });
    }

    let identity = serde_json::json!({
        "contract": request.contract,
        "input": entry.input,
        "conclusion": request.conclusion,
        "evidence": evidence,
        "message": request.message,
    });
    let id = crate::evidence::content_hash(&identity.to_string())[..12].to_owned();
    if let Some(existing) = review.records().find(|r| r.id == id) {
        return Ok(existing.clone());
    }
    let record = Record {
        id,
        at: now()?,
        rev: assessment.rev,
        contract: request.contract.clone(),
        input: entry.input.clone(),
        conclusion: request.conclusion,
        evidence,
        message: request.message.clone(),
        model: assessment.conclusion.as_ref().map(|c| c.status),
    };
    review.append(Entry::Record(record.clone()))?;
    Ok(record)
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Full => "full",
        Mode::Diff => "diff",
    }
}

fn scope_name(scope: RequestedScope) -> &'static str {
    match scope {
        RequestedScope::FullContract => "the full contract",
        RequestedScope::SelectedSubjects => "selected subjects",
    }
}
