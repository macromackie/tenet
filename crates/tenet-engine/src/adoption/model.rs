//! Results and identities reported by adoption commands.

use alloc::collections::BTreeMap;
use core::iter;
use std::path::PathBuf;

use serde::Serialize;

use super::{Kind, Rule};

/// The library entry an installed copy came from, written as its `contract` or `skill` name.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Contract(String),
    Skill(String),
}

impl Origin {
    /// Return the local name of the installed copy.
    #[must_use]
    #[inline]
    pub fn name(&self) -> &str {
        match self {
            Self::Contract(name) | Self::Skill(name) => name,
        }
    }

    /// Return whether the copy is a contract or a skill.
    #[must_use]
    #[inline]
    pub const fn read_kind(&self) -> Kind {
        match self {
            Self::Contract(_) => Kind::Contracts,
            Self::Skill(_) => Kind::Skills,
        }
    }
}

/// What an add installed, and where.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Debug, Serialize)]
pub struct Added {
    /// The lock key: a contract's CONTRACT.md or a skill's folder.
    pub path: PathBuf,
    pub name: String,
    pub source: String,
    #[serde(flatten)]
    pub origin: Origin,
    #[serde(rename = "ref")]
    pub reference: String,
    pub commit: String,
    pub written: bool,
}

/// Whether an installed copy is unchanged, edited, or missing locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalState {
    Edited,
    Missing,
    Unchanged,
}

/// How a release changes one installed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Added,
    Changed,
    Current,
    Removed,
}

/// The newest release an installed copy can move to.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Debug, Serialize)]
pub struct Newest {
    #[serde(rename = "ref")]
    pub reference: String,
    /// The CONTRACT.md or SKILL.md, or the whole copy when upstream removed it.
    pub change: Change,
    /// The other files upstream changed, added, or removed, relative to the copy's folder.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<PathBuf, Change>,
    /// The highest release of each major crossed, oldest first; `update` stops at the first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub majors: Vec<String>,
}

/// One installed copy's lock entry, local state, and newest release.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Debug, Serialize)]
pub struct Status {
    /// The lock key: a contract's CONTRACT.md or a skill's folder.
    pub path: PathBuf,
    /// The local name; `None` when the CONTRACT.md or SKILL.md is missing.
    pub name: Option<String>,
    pub source: String,
    #[serde(flatten)]
    pub origin: Origin,
    #[serde(rename = "ref")]
    pub reference: String,
    /// The CONTRACT.md or SKILL.md.
    pub state: LocalState,
    /// The other locked files that are edited or missing, relative to the copy's folder.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<PathBuf, LocalState>,
    /// `None` when checked offline.
    pub newest: Option<Newest>,
}

impl Status {
    /// True when a file is missing, a read-only file is edited, or upstream changed the copy.
    #[must_use]
    #[inline]
    pub fn needs_attention(&self) -> bool {
        let kind = self.origin.read_kind();
        self.state == LocalState::Missing
            || self.files.iter().any(|(path, state)| {
                *state == LocalState::Missing || kind.derive_rule(path) == Rule::ReadOnly
            })
            || self
                .newest
                .as_ref()
                .is_some_and(|newest| newest.change != Change::Current || !newest.files.is_empty())
    }
}

/// What an update did to one installed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    Added,
    Conflict { regions: u32 },
    Merged,
    Removed,
    Replaced,
    Unchanged,
}

/// What an update did to one installed copy and its files.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Debug, Serialize)]
pub struct Updated {
    pub path: PathBuf,
    pub from: String,
    pub to: String,
    /// The CONTRACT.md or SKILL.md, or the whole copy when upstream removed it.
    #[serde(flatten)]
    pub outcome: Outcome,
    /// The other files that did not stay unchanged, relative to the copy's folder.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<PathBuf, Outcome>,
}

impl Updated {
    /// True when the update wrote a conflict or upstream removed the copy or one of its files.
    #[must_use]
    #[inline]
    pub fn needs_attention(&self) -> bool {
        iter::once(&self.outcome)
            .chain(self.files.values())
            .any(|outcome| {
                match outcome {
                    Outcome::Conflict { .. } | Outcome::Removed => true,
                    Outcome::Added | Outcome::Merged | Outcome::Replaced | Outcome::Unchanged => {
                        false
                    },
                }
            })
    }
}

/// A release of a library copy: its reference, commit, and text.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Debug, Serialize)]
pub struct Version {
    #[serde(rename = "ref")]
    pub reference: String,
    pub commit: String,
    pub text: String,
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
/// The texts behind an update, the CONTRACT.md or SKILL.md rendered with the local name.
#[derive(Debug, Serialize)]
pub struct Comparison {
    pub path: PathBuf,
    pub base: Version,
    /// `None` when upstream removed the copy.
    pub upstream: Option<Version>,
    pub local: String,
    /// The other files that differ between the three versions.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileComparison>,
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
/// One other file of a copy at the locked release, the newest release, and locally. Bytes that are
/// not UTF-8 read as replacement characters.
#[derive(Debug, Serialize)]
pub struct FileComparison {
    /// Relative to the copy's folder.
    pub path: PathBuf,
    /// `None` when the locked release lacks it, so upstream added it.
    pub base: Option<String>,
    /// `None` when the newest release lacks it.
    pub upstream: Option<String>,
    /// `None` when the local file is missing.
    pub local: Option<String>,
}

/// What `validate` found recorded in the lock.
#[derive(Clone, Copy, Debug)]
pub struct Recorded {
    pub contracts: usize,
    pub skills: usize,
}
