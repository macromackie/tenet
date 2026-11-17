mod base;
mod current;
mod reader;

use alloc::sync::Arc;
use std::{io, path::Path};

use reader::ReadSource;

use super::CaptureError;
use crate::{Change, InputLimit, Plan};

enum Side {
    Absent,
    Captured(ReadSource),
    Unavailable(Arc<CaptureError>),
}

impl Side {
    fn into_text(self) -> Option<String> {
        match self {
            Self::Captured(source) => source.text.ok(),
            Self::Absent | Self::Unavailable(_) => None,
        }
    }

    fn read_error(&self) -> Option<Arc<CaptureError>> {
        match self {
            Self::Captured(source) => source.text.as_ref().err().cloned(),
            Self::Unavailable(error) => Some(Arc::clone(error)),
            Self::Absent => None,
        }
    }

    fn read_hash(&self) -> Option<String> {
        match self {
            Self::Captured(source) => Some(source.hash.clone()),
            Self::Absent | Self::Unavailable(_) => None,
        }
    }

    fn read_remaining_retention(&self, limit: usize) -> usize {
        match self {
            Self::Absent => limit,
            Self::Captured(source) => {
                source
                    .text
                    .as_ref()
                    .map_or(0, |text| limit.saturating_sub(text.len()))
            },
            Self::Unavailable(_) => 0,
        }
    }
}

#[derive(Debug)]
pub(super) struct CapturedSource {
    pub after: Option<String>,
    pub before: Option<String>,
    pub change: Result<Change, Arc<CaptureError>>,
    pub has_complete_identity: bool,
}

impl CapturedSource {
    pub(super) fn drop_payload(&mut self, bytes: u64, limit: usize) {
        self.change = Err(Arc::new(CaptureError::Limit(InputLimit::new(
            "aggregate captured source",
            bytes,
            limit,
        ))));
    }

    pub(super) async fn read(plan: &Plan, path: &Path, retain_bytes: usize) -> Self {
        let before = match &plan.base {
            Some(base) => {
                base::read_base(
                    &plan.root,
                    base,
                    plan.previous_paths
                        .get(path)
                        .map_or(path, |value| value.as_path()),
                    retain_bytes,
                )
                .await
            },
            None => Side::Absent,
        };
        let after_limit = before.read_remaining_retention(retain_bytes);
        let after = current::read_current(&plan.root, path, after_limit, plan.base.is_some()).await;
        let has_complete_identity =
            !matches!(&before, Side::Unavailable(_)) && !matches!(&after, Side::Unavailable(_));
        let before_hash = before.read_hash();
        let after_hash = after.read_hash();
        let unavailable = match (&before, &after) {
            (Side::Unavailable(error), _) | (_, Side::Unavailable(error)) => {
                Some(Arc::clone(error))
            },
            (Side::Absent | Side::Captured(_), Side::Absent | Side::Captured(_)) => {
                before.read_error().or_else(|| after.read_error())
            },
        };
        let change = if let Some(error) = unavailable {
            Err(error)
        } else {
            let mut change = Change::new(path.to_owned(), before.into_text(), after.into_text());
            if let Some(previous) = plan.previous_paths.get(path) {
                change.previous_path = Some(previous.clone());
                change.kind = crate::ChangeKind::Renamed;
            }
            Ok(change)
        };
        Self {
            after: after_hash,
            before: before_hash,
            change,
            has_complete_identity,
        }
    }

    pub(super) fn read_source_bytes(&self) -> usize {
        self.change.as_ref().map_or(0, |change| {
            change
                .before
                .as_ref()
                .map_or(0, String::len)
                .saturating_add(change.after.as_ref().map_or(0, String::len))
        })
    }
}

/// Bounded citation capture. A large UTF-8 file retains its exact hash and line count, not its
/// text.
pub(crate) struct RecordSource {
    pub hash: String,
    pub lines: usize,
    pub text: Option<String>,
}

pub(crate) async fn read_record_source(
    root: &Path,
    path: &Path,
    retain_bytes: usize,
) -> Result<RecordSource, crate::SourceError> {
    match current::capture(root, path, retain_bytes, false).await? {
        Side::Captured(source) if source.is_utf8 => {
            Ok(RecordSource {
                hash: source.hash,
                lines: source.line_count,
                text: source.text.ok(),
            })
        },
        Side::Captured(_) => Err(crate::Unreadable::NotUtf8.into()),
        Side::Absent => {
            Err(crate::SourceError::Read {
                path: path.to_owned(),
                source: io::Error::from(io::ErrorKind::NotFound),
            })
        },
        Side::Unavailable(_) => Err(crate::SourceError::ChangedDuringRead(path.to_owned())),
    }
}
