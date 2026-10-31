//! Installing shared contracts and skills through `.tenet.lock`, and taking their upstream changes.
//!
//! Checking reads only the lock's separate `projects`, through discovery. Only these operations
//! touch the network, and nothing persists outside the project.

mod add;
mod commands;
mod error;
mod file_errors;
mod kind;
mod model;
mod unit;
mod upstream;

use alloc::collections::{BTreeMap, btree_map::Entry};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub use add::{AddRequest, Destination, SkillDestination, SkillRequest, add, add_skill};
pub use commands::{diff, list_outdated, remove, update, validate_lock};
#[expect(
    clippy::module_name_repetitions,
    reason = "Callers name it from the public `adoption` module, beside the other crates' error \
              types."
)]
pub use error::AdoptionError;
pub use file_errors::HashError;
pub use kind::Kind;
use kind::Rule;
pub use model::{
    Added,
    Change,
    Comparison,
    FileComparison,
    LocalState,
    Newest,
    Origin,
    Outcome,
    Recorded,
    Status,
    Updated,
    Version,
};
use tenet_contracts::Lock;
use unit::{Unit, decode_text, find_obstruction, read, write};
use upstream::{File, Files, Revision, Upstream, merge};

use crate::git;
pub use crate::lock::LockFileError;

/// Clones each source at most once per command.
struct Libraries {
    clones: BTreeMap<String, Upstream>,
    sources: BTreeMap<String, String>,
}

impl Libraries {
    fn new(lock: &Lock) -> Self {
        Self {
            clones: BTreeMap::new(),
            sources: lock.sources.clone(),
        }
    }

    fn read_upstream(&mut self, source: &str) -> Result<&Upstream, AdoptionError> {
        match self.clones.entry(source.to_owned()) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => {
                let url = self
                    .sources
                    .get(source)
                    .ok_or_else(|| AdoptionError::UnknownLockSource(source.to_owned()))?;
                Ok(entry.insert(Upstream::download(url)?))
            },
        }
    }
}

/// Read a text file; `path` is relative to the project root.
fn read_text(root: &Path, path: &Path) -> Result<String, AdoptionError> {
    fs::read_to_string(root.join(path)).map_err(|source| {
        AdoptionError::Read {
            path: path.to_owned(),
            source,
        }
    })
}

/// Whether Git reports uncommitted changes to any of `paths`, which are relative to the project
/// root.
fn has_uncommitted(root: &Path, kind: Kind, paths: &[PathBuf]) -> Result<bool, AdoptionError> {
    let texts = paths
        .iter()
        .map(|path| path.to_str().ok_or(AdoptionError::NotUtf8Path))
        .collect::<Result<Vec<_>, _>>()?;
    let status = git::run_git(
        root,
        &[&["status", "--porcelain", "--"][..], &texts].concat(),
    )
    .map_err(|source| AdoptionError::NeedsCheckout { kind, source })?;
    Ok(!status.is_empty())
}

/// Refuse to record paths that Git ignores, since Git would never commit them and every other
/// clone would fail `tenet validate`. `paths` are relative to the project root.
fn refuse_ignored(root: &Path, paths: &[PathBuf]) -> Result<(), AdoptionError> {
    if let Some(ignored) = git::find_ignored(root, paths).map_err(AdoptionError::NeedsGit)? {
        return Err(AdoptionError::Ignored {
            path: ignored.path,
            rule: ignored.rule,
        });
    }
    Ok(())
}

fn parse_spec(spec: &str) -> Result<(&str, &str, Option<&str>), AdoptionError> {
    let (source, rest) = spec.split_once('/').ok_or(AdoptionError::SpecFormat)?;
    let (name, reference) = match rest.split_once('@') {
        Some((name, reference)) => (name, Some(reference)),
        None => (rest, None),
    };
    if !(tenet_contracts::is_kebab(source) && tenet_contracts::is_kebab(name)) {
        return Err(AdoptionError::SpecNames);
    }
    Ok((source, name, reference))
}

fn select(lock: &Lock, kind: Kind, paths: &[PathBuf]) -> Result<Vec<Unit>, AdoptionError> {
    if paths.is_empty() {
        return Unit::list_all(lock, kind);
    }
    paths
        .iter()
        .map(|path| Unit::find(lock, kind, &kind.derive_key(path)?))
        .collect()
}

#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "The remaining variants take no action here, and listing each would repeat the enum."
)]
fn make_relative(path: &Path) -> Result<PathBuf, AdoptionError> {
    let mut normal = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(segment) => normal.push(segment),
            Component::CurDir => {},
            _ => return Err(AdoptionError::NotRelative(path.to_owned())),
        }
    }
    Ok(normal)
}
