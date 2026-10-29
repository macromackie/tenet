//! An agent's conclusion for a contract's current input, validated before it is appended and
//! again whenever the log is read.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use super::{
    Assessment,
    CapturedFile,
    Entry,
    Record,
    Review,
    ReviewError,
    Revision,
    Role,
    Span,
    read_clock,
    resolve_current,
};
use crate::{
    ContractState,
    ContractStatus,
    Mode,
    Pending,
    RequestedScope,
    SpanRequest,
    Status,
    evidence,
    plan,
};

/// The longest message a record keeps, in bytes.
pub(super) const MAX_MESSAGE_BYTES: usize = 4096;
/// How many changed ranges a refusal lists.
const LISTED_CHANGES: usize = 10;
/// The control characters a record message may hold.
const MESSAGE_CONTROLS: [char; 2] = ['\n', '\t'];
/// The conclusions a record cannot reach while a check still fails.
const UNFAVORABLE: [ContractStatus; 2] = [ContractStatus::Failed, ContractStatus::Unresolved];

/// What `tenet record` asks to record for one contract.
#[derive(Debug)]
pub struct RecordRequest {
    pub conclusion: ContractStatus,
    pub contract: String,
    pub evidence: Vec<SpanRequest>,
    pub message: String,
    /// The selected paths of the check to settle; empty for the contract's only selection.
    pub paths: Vec<PathBuf>,
}

/// A record that now settles its input, and the earlier record for that input it replaced.
#[derive(Debug)]
pub struct Recorded {
    pub record: Record,
    pub replaces: Option<Record>,
}

/// Conclusions a record may state, which are those a check of the same mode and scope can reach.
#[must_use]
#[inline]
pub const fn list_conclusions(mode: Mode, scope: RequestedScope) -> &'static [ContractStatus] {
    use ContractStatus::{Clear, Failed, Preserved, Unaffected, Unresolved, Verified};
    match (mode, scope) {
        (_, RequestedScope::SelectedSubjects) => &[Clear, Failed, Unresolved],
        (Mode::Full, RequestedScope::FullContract) => &[Verified, Failed, Unresolved],
        (Mode::Diff, RequestedScope::FullContract) => &[Preserved, Unaffected, Failed, Unresolved],
    }
}

/// Bind a conclusion to the input the latest check assessed. Refuse it when the contract's scoped
/// files changed since, so a verdict never attaches to code its author did not read.
#[inline]
#[expect(
    clippy::future_not_send,
    reason = "The checker drives this future on one task and never moves it across threads."
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
pub async fn record(
    root: &Path,
    state: &Path,
    request: &RecordRequest,
) -> Result<Recorded, ReviewError> {
    let review = Review::open_existing(state)?;
    let current = resolve_current(root, &review, None, Some(&request.contract)).await?;
    let entry = current.select(&request.contract, &request.paths)?;
    let Some(assessment) = entry.assessment else {
        match entry.state {
            ContractState::Pending {
                reason: Pending::NotChecked,
            } => return Err(ReviewError::NotChecked(request.contract.clone())),
            _ => {
                return Err(ReviewError::Stale {
                    contract: request.contract.clone(),
                    rev: entry.selection.rev,
                });
            },
        }
    };

    let revision = review.find_revision(assessment.rev)?;
    let canonical = root.canonicalize().map_err(ReviewError::InvalidRoot)?;
    let mut evidence = Vec::new();
    for span in &request.evidence {
        // `./src/a.rs` names `src/a.rs`.
        let path: PathBuf = (span.path.components())
            .filter(|part| *part != Component::CurDir)
            .collect();
        if !plan::is_source(&canonical, &path).map_err(ReviewError::Inspect)? {
            return Err(ReviewError::NotSource(span.path.clone()));
        }
        let (_, file) = select_scoped(revision, assessment, &request.contract, &path)?;
        let full = canonical.join(&path);
        if !full.exists() && file.after.is_none() {
            if span.lines.is_some() {
                return Err(ReviewError::DeletedLines(path));
            }
            let blob =
                (file.before.clone()).ok_or_else(|| ReviewError::NoContents(path.clone()))?;
            evidence.push(Span {
                path,
                lines: None,
                blob,
            });
            continue;
        }
        let text = fs::read_to_string(&full).map_err(|source| {
            ReviewError::ReadEvidence {
                path: path.clone(),
                source,
            }
        })?;
        let count = text.lines().count();
        if let Some((start, end)) = span.lines
            && end > count
        {
            return Err(ReviewError::PastEnd {
                path,
                start,
                end,
                count,
            });
        }
        evidence.push(Span {
            path,
            lines: span.lines.map(Into::into),
            blob: review.keep(&text)?,
        });
    }

    let record = Record {
        id: compute_id(
            &request.contract,
            &entry.input,
            request.conclusion,
            &evidence,
            &request.message,
        ),
        at: read_clock()?,
        rev: assessment.rev,
        contract: request.contract.clone(),
        input: entry.input.clone(),
        conclusion: request.conclusion,
        evidence,
        message: request.message.clone(),
        model: assessment
            .conclusion
            .as_ref()
            .map(|conclusion| conclusion.status),
    };
    validate(&record, assessment, revision, &review)?;
    // The latest record for an input settles it, so repeating that record changes nothing.
    let replaces = (review.list_records().rev())
        .find(|existing| existing.contract == record.contract && existing.input == record.input)
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
#[expect(
    clippy::indexing_slicing,
    reason = "`min` keeps the end at most the slice length."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
#[expect(
    clippy::else_if_without_else,
    reason = "`needless_else` rejects the empty `else` this lint asks for."
)]
pub(super) fn validate(
    record: &Record,
    assessment: &Assessment,
    revision: &Revision,
    review: &Review,
) -> Result<(), ReviewError> {
    let mode = revision.mode;
    let allowed = list_conclusions(mode, assessment.scope);
    if !allowed.contains(&record.conclusion) {
        return Err(ReviewError::Unfit {
            conclusion: record.conclusion,
            mode,
            scope: assessment.scope,
            allowed,
        });
    }
    if record.message.trim().is_empty() {
        return Err(ReviewError::NoMessage);
    }
    if record.message.len() > MAX_MESSAGE_BYTES {
        return Err(ReviewError::LongMessage(record.message.len()));
    }
    if record.message.chars().any(|character| {
        (character.is_control() && !MESSAGE_CONTROLS.contains(&character))
            || matches!(
                character,
                '\u{61c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}'
            )
            || matches!(character, '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}')
    }) {
        return Err(ReviewError::MessageCharacters);
    }
    // A diff check that selected no file, as when the diff changes only the contract, has no
    // code to cite.
    let nothing_selected = mode == Mode::Diff && assessment.selected.is_empty();
    if record.conclusion != ContractStatus::Unresolved
        && !nothing_selected
        && record.evidence.is_empty()
    {
        return Err(ReviewError::NoEvidence(record.conclusion));
    }
    for span in &record.evidence {
        let (role, file) = select_scoped(revision, assessment, &record.contract, &span.path)?;
        if let Some([start, end]) = span.lines {
            if start < 1 {
                return Err(ReviewError::LineZero {
                    path: span.path.clone(),
                    start,
                    end,
                });
            }
            if start > end {
                return Err(ReviewError::Reversed {
                    path: span.path.clone(),
                    start,
                    end,
                });
            }
        }
        // The check supplied only these lines of a supporting excerpt, so only they are evidence.
        // A selected file is the subject itself, whatever lines stood in for its contents.
        let supplied = |[start, end]: [usize; 2]| {
            (file.excerpt.iter()).any(|&[first, last]| first <= start && end <= last)
        };
        if !(role == Role::Selected || file.excerpt.is_empty() || span.lines.is_some_and(supplied))
        {
            return Err(ReviewError::OutsideExcerpt {
                rev: assessment.rev,
                excerpt: file.excerpt.clone(),
                path: span.path.clone(),
            });
        }
    }

    // A dispute must answer the files the model flagged, not cite something beside them.
    let flagged: Vec<&str> = assessment
        .results
        .iter()
        .filter(|result| result.status == Status::Fail)
        .map(|result| result.path.as_str())
        .collect();
    let favorable = !UNFAVORABLE.contains(&record.conclusion);
    if favorable
        && !flagged.is_empty()
        && !(record.evidence.iter())
            .any(|span| flagged.iter().any(|path| span.path == Path::new(path)))
    {
        return Err(ReviewError::Unanswered {
            flagged: flagged.into_iter().map(str::to_owned).collect(),
            conclusion: record.conclusion,
        });
    }

    // In a diff, a favorable record answers the change itself: it cites a line the change
    // wrote, or a file it deleted or emptied, whenever the change touched a selected file. With
    // nothing selected, the contract's full check carries the evidence.
    if favorable && mode == Mode::Diff {
        let files: Vec<_> = revision.list_files(assessment).collect();
        if nothing_selected {
            if !is_verified(review, &record.contract, &assessment.contract_hash) {
                return Err(ReviewError::NothingSelected {
                    contract: record.contract.clone(),
                    conclusion: record.conclusion,
                });
            }
        } else if (files.iter()).any(|(role, file)| *role == Role::Selected && file.is_changed()) {
            let cites = |span: &Span| {
                (files.iter())
                    .any(|(_, file)| file.path == span.path && file.is_citing_change(span.lines))
            };
            if !record.evidence.iter().any(cites) {
                let citations: Vec<_> = (files.iter())
                    .flat_map(|(_, file)| file.change_citations())
                    .collect();
                let remaining = match citations.len().saturating_sub(LISTED_CHANGES) {
                    0 => String::new(),
                    unlisted => format!(", and {unlisted} more; tenet show lists them"),
                };
                return Err(ReviewError::UncitedChange {
                    conclusion: record.conclusion,
                    citations: format!(
                        "{:?}{remaining:?}",
                        citations[..citations.len().min(LISTED_CHANGES)].join(", ")
                    ),
                });
            }
        }
    }

    if record.id
        != compute_id(
            &record.contract,
            &record.input,
            record.conclusion,
            &record.evidence,
            &record.message,
        )
    {
        return Err(ReviewError::IdMismatch);
    }
    Ok(())
}

/// Whether a full check of this exact contract text settled as verified, by the latest record for
/// its input or else by the model.
fn is_verified(review: &Review, contract: &str, contract_hash: &str) -> bool {
    (review.list_assessments())
        .filter(|assessment| {
            assessment.contract == contract && assessment.contract_hash == contract_hash
        })
        .filter(|assessment| assessment.scope == RequestedScope::FullContract)
        .filter(|assessment| {
            (review.find_revision(assessment.rev)).is_ok_and(|revision| revision.mode == Mode::Full)
        })
        .any(|assessment| {
            let recorded = (review.list_records().rev())
                .find(|record| record.contract == contract && record.input == assessment.input);
            let conclusion = match recorded {
                Some(record) => Some(record.conclusion),
                None => {
                    assessment
                        .conclusion
                        .as_ref()
                        .map(|conclusion| conclusion.status)
                },
            };
            conclusion == Some(ContractStatus::Verified)
        })
}

/// The scoped file a citation names, and its role; evidence must come from what the check
/// assessed.
fn select_scoped<'review>(
    revision: &'review Revision,
    assessment: &'review Assessment,
    contract: &str,
    path: &Path,
) -> Result<(Role, &'review CapturedFile), ReviewError> {
    let mut files = revision.list_files(assessment);
    files.find(|(_, file)| file.path == path).ok_or_else(|| {
        ReviewError::NotScoped {
            path: path.to_owned(),
            rev: assessment.rev,
            contract: contract.to_owned(),
            files: revision
                .list_files(assessment)
                .map(|(_, file)| file.path.display().to_string())
                .collect(),
        }
    })
}

/// A record's identity: the same conclusion, evidence, and message for the same input is the same
/// record.
fn compute_id(
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
    let mut id = evidence::hash_content(&identity.to_string());
    id.truncate(12);
    id
}
