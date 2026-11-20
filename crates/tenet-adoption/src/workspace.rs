//! Shared workspace helpers for adoption commands: upstream cache, Git status, and spec parsing.

use alloc::collections::{BTreeMap, btree_map::Entry};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use tenet_contracts::Lock;
use tenet_engine::{find_ignored, run_git};

use crate::{error::AdoptionError, kind::Kind, unit::Unit, upstream::Upstream};

/// Clones each source at most once per command.
pub(crate) struct Libraries {
    clones: BTreeMap<String, Upstream>,
    sources: BTreeMap<String, String>,
}

impl Libraries {
    pub(crate) fn new(lock: &Lock) -> Self {
        Self {
            clones: BTreeMap::new(),
            sources: lock.sources.clone(),
        }
    }

    pub(crate) fn read_upstream(&mut self, source: &str) -> Result<&Upstream, AdoptionError> {
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

/// The lock key for a command's path: a contract's CONTRACT.md, given it or its folder, or a
/// skill's folder, given it or its SKILL.md.
pub(crate) fn derive_key(kind: Kind, path: &Path) -> Result<PathBuf, AdoptionError> {
    let relative_path = make_relative(path)?;
    let named = relative_path
        .file_name()
        .is_some_and(|name| name == kind.name_document());
    Ok(match (kind, named) {
        (Kind::Contracts, true) | (Kind::Skills, false) => relative_path,
        (Kind::Contracts, false) => relative_path.join(kind.name_document()),
        (Kind::Skills, true) => {
            relative_path
                .parent()
                .ok_or(AdoptionError::NoSkillFolder)?
                .to_owned()
        },
    })
}

/// Whether Git reports uncommitted changes to any of `paths`, which are relative to the project
/// root.
pub(crate) fn has_uncommitted(
    root: &Path,
    kind: Kind,
    paths: &[PathBuf],
) -> Result<bool, AdoptionError> {
    let texts = paths
        .iter()
        .map(|path| path.to_str().ok_or(AdoptionError::NotUtf8Path))
        .collect::<Result<Vec<_>, _>>()?;
    let status = run_git(
        root,
        &[&["status", "--porcelain", "--"][..], &texts].concat(),
    )
    .map_err(|source| AdoptionError::NeedsCheckout { kind, source })?;
    Ok(!status.is_empty())
}

#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "The remaining variants take no action here, and listing each would repeat the enum."
)]
pub(crate) fn make_relative(path: &Path) -> Result<PathBuf, AdoptionError> {
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

pub(crate) fn parse_spec(spec: &str) -> Result<(&str, &str, Option<&str>), AdoptionError> {
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

/// Read a text file; `path` is relative to the project root.
pub(crate) fn read_text(root: &Path, path: &Path) -> Result<String, AdoptionError> {
    fs::read_to_string(root.join(path)).map_err(|source| {
        AdoptionError::Read {
            path: path.to_owned(),
            source,
        }
    })
}

/// Refuse to record paths that Git ignores, since Git would never commit them and every other
/// clone would fail `tenet validate`. `paths` are relative to the project root.
pub(crate) fn refuse_ignored(root: &Path, paths: &[PathBuf]) -> Result<(), AdoptionError> {
    if let Some(ignored) = find_ignored(root, paths).map_err(AdoptionError::NeedsGit)? {
        return Err(AdoptionError::Ignored {
            path: ignored.path,
            rule: ignored.rule,
        });
    }
    Ok(())
}

pub(crate) fn select(
    lock: &Lock,
    kind: Kind,
    paths: &[PathBuf],
) -> Result<Vec<Unit>, AdoptionError> {
    if paths.is_empty() {
        return Unit::list_all(lock, kind);
    }
    paths
        .iter()
        .map(|path| Unit::find(lock, kind, &derive_key(kind, path)?))
        .collect()
}
