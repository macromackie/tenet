//! A revision captures a selection and maps its assessments to shared file identities.

use alloc::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Assessment, CapturedFile, Role};
use crate::Mode;

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
/// One `tenet check`. Its selection lets later commands recapture exactly what it assessed.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Revision {
    pub rev: u64,
    /// Unix seconds.
    pub at: u64,
    /// The branch and base this revision belongs to, for display.
    pub review: String,
    pub base: Option<String>,
    pub paths: Vec<PathBuf>,
    pub contract: Option<String>,
    pub context: Vec<crate::SpanRequest>,
    pub mode: Mode,
    pub base_oid: Option<String>,
    pub head_oid: Option<String>,
    /// The provider and model that answered; absent for `check --no-model`.
    pub model: Option<String>,
    /// The confidence below which the model's answers were routed to unresolved; absent for
    /// `check --no-model` and for revisions written before Tenet recorded it.
    pub min_confidence: Option<f64>,
    pub tenet: String,
    /// Every file this revision's assessments cover, each once. Format 1 listed them in each
    /// assessment instead.
    #[serde(default)]
    pub files: Vec<CapturedFile>,
}

impl Revision {
    /// Whether this revision's path selection includes everything `earlier` selected, so its
    /// assessment of a contract replaces `earlier`'s. No paths select everything.
    #[must_use]
    #[inline]
    pub fn is_covering(&self, earlier: &Self) -> bool {
        let selected = build_selectors(&self.paths);
        let earlier_selectors = build_selectors(&earlier.paths);
        selected.is_empty()
            || (!earlier_selectors.is_empty()
                && earlier_selectors
                    .iter()
                    .all(|path| selected.iter().any(|selector| path.starts_with(selector))))
    }

    /// Revisions against the same base commit review the same change, whichever paths and
    /// contracts they selected. Full checks have no base and form one review.
    #[must_use]
    #[inline]
    pub fn is_same_review(&self, other: &Self) -> bool {
        self.base_oid == other.base_oid
    }

    /// Whether these are the revision's selected paths, however they were spelled.
    #[must_use]
    #[inline]
    pub fn is_selecting(&self, paths: &[PathBuf]) -> bool {
        build_selectors(&self.paths) == build_selectors(paths)
    }

    /// The files one of this revision's assessments covers: its selected files, then its
    /// supporting files.
    #[expect(
        clippy::indexing_slicing,
        reason = "Assessment file indexes point into this log's `files`, which the store \
                  validates on read."
    )]
    #[inline]
    pub fn list_files<'log>(
        &'log self,
        assessment: &'log Assessment,
    ) -> impl Iterator<Item = (Role, &'log CapturedFile)> {
        let selected = assessment
            .selected
            .iter()
            .map(|&index| (Role::Selected, index));
        let support = assessment
            .support
            .iter()
            .map(|&index| (Role::Support, index));
        selected
            .chain(support)
            .map(|(role, index)| (role, &self.files[index]))
    }
}

fn build_selectors(paths: &[PathBuf]) -> BTreeSet<PathBuf> {
    paths
        .iter()
        .map(|path| {
            path.components()
                .filter(|part| *part != Component::CurDir)
                .collect::<PathBuf>()
        })
        .filter(|path| path != Path::new(""))
        .collect()
}
