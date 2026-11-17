//! Bind an agent's conclusion to captured input before appending it to the log.

use std::path::{Component, Path, PathBuf};

use super::{RecordRequest, Recorded, UNFAVORABLE, compute_id, select_scoped, validate};
use crate::{
    ContractState,
    ContractStatus,
    Pending,
    evidence::read_record_source,
    plan,
    review::{
        Entry,
        MAX_BLOB_BYTES,
        Record,
        Review,
        ReviewError,
        Role,
        Span,
        read_clock,
        resolve_current,
    },
};

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

    if !entry.has_complete_identity && request.conclusion != ContractStatus::Unresolved {
        return Err(ReviewError::IdentityUnavailable(request.contract.clone()));
    }
    let revision = review.find_revision(assessment.rev)?;
    // Without retained diff text there are no exact changed-line citations. A favorable record
    // must wait for an admissible diff or a full check, rather than cite an unrelated line.
    if entry.mode == crate::Mode::Diff
        && !UNFAVORABLE.contains(&request.conclusion)
        && let Some(scoped) = entry.files.iter().find(|scoped| {
            scoped.role == Role::Selected
                && scoped.file.error.is_some()
                && scoped.file.after.is_some()
                && scoped.file.before != scoped.file.after
        })
    {
        return Err(ReviewError::NoContents(scoped.file.path.clone()));
    }
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
        let source = read_record_source(&canonical, &path, MAX_BLOB_BYTES).await?;
        if file.after.as_deref() != Some(source.hash.as_str()) {
            return Err(ReviewError::Stale {
                contract: request.contract.clone(),
                rev: entry.selection.rev,
            });
        }
        let count = source.lines;
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
        let blob = match source.text {
            Some(text) => review.keep(&text)?,
            None => source.hash,
        };
        evidence.push(Span {
            path,
            lines: span.lines.map(Into::into),
            blob,
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
