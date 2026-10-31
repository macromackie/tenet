use alloc::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use tenet_contracts::Contract;

use crate::change::Excerpt;

/// The paths, contracts, and base that a command asks to check.
#[derive(Debug, Default)]
pub struct Selection {
    pub base: Option<String>,
    pub contract: Option<String>,
    pub paths: Vec<PathBuf>,
}

impl Selection {
    /// Whether a selected path is relative and contains no parent-directory step.
    #[must_use]
    #[inline]
    pub fn is_relative_path(path: &Path) -> bool {
        path.components().all(|component| {
            match component {
                Component::Normal(_) | Component::CurDir => true,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir => false,
            }
        })
    }
}

/// The contracts and files a check reads, and the files it leaves out.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Debug, Serialize)]
pub struct Plan {
    pub root: PathBuf,
    pub contracts: Vec<Contract>,
    pub files: Vec<PathBuf>,
    pub deleted: Vec<PathBuf>,
    pub base: Option<String>,
    pub head: Option<String>,
    pub mode: crate::Mode,
    /// Every discovered file, contract documents included; each contract governs part of it.
    pub inventory: Vec<PathBuf>,
    pub candidate_files: Vec<PathBuf>,
    pub selected_paths: Vec<PathBuf>,
    /// Selected paths that yield no subject for any planned contract.
    pub excluded_paths: Vec<ExcludedPath>,
    pub previous_paths: BTreeMap<PathBuf, PathBuf>,
    pub context: Vec<crate::Change>,
    /// `context` files supplied as line ranges instead of whole; the rest are supplied whole.
    pub excerpts: BTreeMap<PathBuf, Excerpt>,
    /// The contract documents a diff changes, directly or through another file in their folder,
    /// such as an installed asset.
    pub contract_changes: Vec<PathBuf>,
    /// How much of a diff's selected change the contracts govern; `None` for a full check.
    pub coverage: Option<Coverage>,
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
/// How much of a diff's selected change any contract governs, whichever contracts a check runs.
#[derive(Clone, Debug, Serialize)]
pub struct Coverage {
    /// Selected changed files, contract documents included.
    pub changed_files: usize,
    /// Changed files no contract governs at their current or previous path. Files in a
    /// `.contracts` folder are contract changes, not ungoverned files.
    pub ungoverned_files: Vec<PathBuf>,
    /// Whether a contract governs any changed file or the change edits a contract's folder. It
    /// is false when no contract applies, including when nothing changed.
    pub governed: bool,
}

/// A selected path where a check assesses nothing, and why.
#[derive(Clone, Debug, Serialize)]
pub struct ExcludedPath {
    pub path: PathBuf,
    pub reason: Exclusion,
}

/// Why discovery left a file out of a check.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Exclusion {
    /// Discovery finds no file there: ignore rules, a protected name, or an empty directory.
    NotDiscovered,
    /// Discovery finds files there, but no planned contract governs them.
    NotGoverned,
}
