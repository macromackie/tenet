//! An agent's conclusion for a contract's current input, validated before it is appended and
//! again whenever the log is read.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};

use super::{Assessment, CapturedFile, Entry, Record, Review, Revision, Role, Span, current, now};
use crate::{
    ContractState, ContractStatus, Mode, Pending, RequestedScope, SpanRequest, Status, plan,
};

/// The longest message a record keeps, in bytes.
const MAX_MESSAGE_BYTES: usize = 4096;
/// How many changed ranges a refusal lists.
const LISTED_CHANGES: usize = 10;

pub struct RecordRequest {
    pub contract: String,
    /// The selected paths of the check to settle; empty for the contract's only selection.
    pub paths: Vec<PathBuf>,
    pub conclusion: ContractStatus,
    pub evidence: Vec<SpanRequest>,
    pub message: String,
}

/// A record that now settles its input, and the earlier record for that input it replaced.
pub struct Recorded {
    pub record: Record,
    pub replaces: Option<Record>,
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
pub async fn record(root: &Path, state: &Path, request: &RecordRequest) -> Result<Recorded> {
    let review = Review::existing(state)?;
    let current = current(root, &review, None, Some(&request.contract)).await?;
    let entry = current.select(&request.contract, &request.paths)?;
    let Some(assessment) = entry.assessment else {
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
                entry.selection.rev
            ),
        }
    };

    let revision = review.revision(assessment.rev)?;
    let root = root.canonicalize().context("invalid project root")?;
    let mut evidence = Vec::new();
    for span in &request.evidence {
        // `./src/a.rs` names `src/a.rs`.
        let path: PathBuf = (span.path.components())
            .filter(|part| *part != Component::CurDir)
            .collect();
        ensure!(
            plan::is_source(&root, &path)?,
            "evidence paths must be repository source files: {}",
            span.path.display()
        );
        let (_, file) = scoped(revision, assessment, &request.contract, &path)?;
        let full = root.join(&path);
        if !full.exists() && file.after.is_none() {
            ensure!(
                span.lines.is_none(),
                "{} was deleted; cite it without lines",
                path.display()
            );
            evidence.push(Span {
                blob: file
                    .before
                    .clone()
                    .with_context(|| format!("{} has no captured contents", path.display()))?,
                path,
                lines: None,
            });
            continue;
        }
        let text = fs::read_to_string(&full)
            .with_context(|| format!("cannot read evidence {}", path.display()))?;
        let count = text.lines().count();
        if let Some((start, end)) = span.lines {
            ensure!(
                end <= count,
                "{}:{start}-{end} ends after the file's {count} lines",
                path.display()
            );
        }
        evidence.push(Span {
            path,
            lines: span.lines.map(|(start, end)| [start, end]),
            blob: review.keep(&text)?,
        });
    }

    let record = Record {
        id: id(
            &request.contract,
            &entry.input,
            request.conclusion,
            &evidence,
            &request.message,
        ),
        at: now()?,
        rev: assessment.rev,
        contract: request.contract.clone(),
        input: entry.input.clone(),
        conclusion: request.conclusion,
        evidence,
        message: request.message.clone(),
        model: assessment.conclusion.as_ref().map(|c| c.status),
    };
    validate(&record, assessment, revision, &review)?;
    // The latest record for an input settles it, so repeating that record changes nothing.
    let replaces = (review.records().rev())
        .find(|r| r.contract == record.contract && r.input == record.input)
        .cloned();
    if let Some(latest) = &replaces
        && latest.id == record.id
    {
        return Ok(Recorded {
            record: latest.clone(),
            replaces: None,
        });
    }
    review.append(Entry::Record(record.clone()))?;
    Ok(Recorded { record, replaces })
}

/// What every record must satisfy against the assessment it settles, one of `revision`'s, whether
/// `record` is making it or the log is being read; `review` holds the entries before it.
pub(super) fn validate(
    record: &Record,
    assessment: &Assessment,
    revision: &Revision,
    review: &Review,
) -> Result<()> {
    let mode = revision.mode;
    let allowed = conclusions(mode, assessment.scope);
    ensure!(
        allowed.contains(&record.conclusion),
        "{} does not fit a {} check of {}; record one of: {}",
        record.conclusion.as_str(),
        mode.as_str(),
        assessment.scope.description(),
        allowed
            .iter()
            .map(|c| c.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    ensure!(
        !record.message.trim().is_empty(),
        "state what the evidence shows with --message"
    );
    ensure!(
        record.message.len() <= MAX_MESSAGE_BYTES,
        "the message is {} bytes; the limit is {MAX_MESSAGE_BYTES}",
        record.message.len()
    );
    ensure!(
        !record.message.chars().any(|c| {
            (c.is_control() && c != '\n' && c != '\t')
                || matches!(
                    c,
                    '\u{61c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}'
                )
                || matches!(c, '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}')
        }),
        "the message contains control, bidirectional, or line separator characters; only \
         newlines and tabs are allowed"
    );
    // A diff check that selected no file, as when the diff changes only the contract, has no
    // code to cite.
    let nothing_selected = mode == Mode::Diff && assessment.selected.is_empty();
    ensure!(
        record.conclusion == ContractStatus::Unresolved
            || nothing_selected
            || !record.evidence.is_empty(),
        "cite the code that supports {} with --evidence PATH:START-END",
        record.conclusion.as_str()
    );
    for span in &record.evidence {
        let (role, file) = scoped(revision, assessment, &record.contract, &span.path)?;
        if let Some([start, end]) = span.lines {
            let cited = format!("{}:{start}-{end}", span.path.display());
            ensure!(start >= 1, "{cited}: lines are numbered from 1");
            ensure!(start <= end, "{cited}: the range starts after it ends");
        }
        // The check supplied only these lines of a supporting excerpt, so only they are evidence.
        // A selected file is the subject itself, whatever lines stood in for its contents.
        let supplied = |[start, end]: [usize; 2]| {
            (file.excerpt.iter()).any(|&[first, last]| first <= start && end <= last)
        };
        ensure!(
            role == Role::Selected || file.excerpt.is_empty() || span.lines.is_some_and(supplied),
            "r{} supplied only lines {} of {}; cite lines within them",
            assessment.rev,
            crate::line_ranges(&file.excerpt),
            span.path.display()
        );
    }

    // A dispute must answer the files the model flagged, not cite something beside them.
    let flagged: Vec<&str> = assessment
        .results
        .iter()
        .filter(|result| result.status == Status::Fail)
        .map(|result| result.path.as_str())
        .collect();
    let favorable = !matches!(
        record.conclusion,
        ContractStatus::Failed | ContractStatus::Unresolved
    );
    ensure!(
        !favorable
            || flagged.is_empty()
            || record
                .evidence
                .iter()
                .any(|span| flagged.iter().any(|path| span.path == Path::new(path))),
        "the model flagged {}; cite the lines there that show {}",
        flagged.join(", "),
        record.conclusion.as_str()
    );

    // In a diff, a favorable record answers the change itself: it cites a line the change
    // wrote, or a file it deleted or emptied, whenever the change touched a selected file. With nothing
    // selected, the contract's full check carries the evidence.
    if favorable && mode == Mode::Diff {
        let files: Vec<_> = revision.files(assessment).collect();
        if nothing_selected {
            ensure!(
                verified(review, &record.contract, &assessment.contract_hash),
                "this diff check selected no file {contract} governs, so nothing in it shows {}. \
                 Check the contract in full with `tenet check --contract {contract}`, settle that \
                 check as verified, then record this one",
                record.conclusion.as_str(),
                contract = record.contract
            );
        } else if (files.iter()).any(|(role, file)| *role == Role::Selected && file.is_changed()) {
            let cites = |span: &Span| {
                (files.iter())
                    .any(|(_, file)| file.path == span.path && file.cites_change(span.lines))
            };
            ensure!(record.evidence.iter().any(cites), "{}", {
                let citations: Vec<_> = (files.iter())
                    .flat_map(|(_, file)| file.change_citations())
                    .collect();
                let more = match citations.len().saturating_sub(LISTED_CHANGES) {
                    0 => String::new(),
                    more => format!(", and {more} more; tenet show lists them"),
                };
                format!(
                    "cite a line this change wrote, a line beside it, or a file it deleted or emptied, to show {}: {}{more}",
                    record.conclusion.as_str(),
                    citations[..citations.len().min(LISTED_CHANGES)].join(", ")
                )
            });
        }
    }

    ensure!(
        record.id
            == id(
                &record.contract,
                &record.input,
                record.conclusion,
                &record.evidence,
                &record.message
            ),
        "its id does not match its contents"
    );
    Ok(())
}

/// Whether a full check of this exact contract text settled as verified, by the latest record for
/// its input or else by the model.
fn verified(review: &Review, contract: &str, contract_hash: &str) -> bool {
    (review.assessments())
        .filter(|a| a.contract == contract && a.contract_hash == contract_hash)
        .filter(|a| a.scope == RequestedScope::FullContract)
        .filter(|a| (review.revision(a.rev)).is_ok_and(|revision| revision.mode == Mode::Full))
        .any(|a| {
            let recorded = (review.records().rev())
                .find(|record| record.contract == contract && record.input == a.input);
            let conclusion = match recorded {
                Some(record) => Some(record.conclusion),
                None => a.conclusion.as_ref().map(|c| c.status),
            };
            conclusion == Some(ContractStatus::Verified)
        })
}

/// The scoped file a citation names, and its role; evidence must come from what the check
/// assessed.
fn scoped<'a>(
    revision: &'a Revision,
    assessment: &'a Assessment,
    contract: &str,
    path: &Path,
) -> Result<(Role, &'a CapturedFile)> {
    let mut files = revision.files(assessment);
    files.find(|(_, file)| file.path == path).with_context(|| {
        let files: Vec<_> = revision
            .files(assessment)
            .map(|(_, file)| file.path.display().to_string())
            .collect();
        format!(
            "{} is not a file r{} assessed for {contract}; cite one of: {}",
            path.display(),
            assessment.rev,
            files.join(", ")
        )
    })
}

/// A record's identity: the same conclusion, evidence, and message for the same input is the same
/// record.
fn id(
    contract: &str,
    input: &str,
    conclusion: ContractStatus,
    evidence: &[Span],
    message: &str,
) -> String {
    let identity = serde_json::json!({
        "contract": contract,
        "input": input,
        "conclusion": conclusion,
        "evidence": evidence,
        "message": message,
    });
    crate::evidence::content_hash(&identity.to_string())[..12].to_owned()
}
