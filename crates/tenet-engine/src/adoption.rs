//! Installing shared contracts and skills through `.tenet.lock`, and taking their upstream changes.
//!
//! Checking reads only the lock's separate `projects`, through discovery. Only these operations
//! touch the network, and nothing persists outside the project.

mod add;
mod error;
mod kind;
mod unit;
mod upstream;

use alloc::collections::{BTreeMap, btree_map::Entry};
use core::iter;
use std::{
    fs,
    io::Write as _,
    path::{Component, Path, PathBuf},
};

pub use add::{AddRequest, Destination, SkillDestination, SkillRequest, add, add_skill};
#[expect(
    clippy::module_name_repetitions,
    reason = "Callers name it from the public `adoption` module, beside the other crates' error \
              types."
)]
pub use error::{AdoptionError, HashError, LockFileError};
pub use kind::Kind;
use kind::Rule;
use serde::Serialize;
use tempfile::NamedTempFile;
use tenet_contracts::{LOCK_FILE, Lock, find_conflict_marker, name_of, validate_skill, with_name};
use unit::Unit;
use upstream::{Files, Upstream};

use crate::git;

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

/// List each installed copy of `kind` with its newest release, reading no remote when `offline` is
/// set.
#[inline]
pub fn list_outdated(root: &Path, kind: Kind, offline: bool) -> Result<Vec<Status>, AdoptionError> {
    let lock = read_lock(root)?;
    let mut libraries = Libraries::new(&lock);
    let mut statuses = Vec::new();
    for unit in Unit::list_all(&lock, kind)? {
        let (state, mut files) = unit.split(unit.read_local_states(root)?)?;
        files.retain(|_, file_state| *file_state != LocalState::Unchanged);
        let name = match state {
            LocalState::Missing => None,
            LocalState::Unchanged | LocalState::Edited => {
                Some(name_of(&read_text(root, &unit.name_document())?)?.to_owned())
            },
        };
        let newest = if offline {
            None
        } else {
            let upstream = libraries.read_upstream(&unit.source)?;
            let revision = upstream.find_newest(&unit.reference)?;
            let (change, changed_files) =
                match upstream.read_folder(&revision.commit, kind, unit.origin.name())? {
                    None => (Change::Removed, BTreeMap::new()),
                    Some(next) => {
                        let (change, mut upstream_files) = unit.split(unit.plan_changes(&next)?)?;
                        upstream_files.retain(|_, file_change| *file_change != Change::Current);
                        (change, upstream_files)
                    },
                };
            Some(Newest {
                reference: revision.reference,
                change,
                files: changed_files,
                majors: upstream.list_majors(&unit.reference)?,
            })
        };
        statuses.push(Status {
            path: unit.key,
            name,
            source: unit.source,
            origin: unit.origin,
            reference: unit.reference,
            state,
            files,
            newest,
        });
    }
    Ok(statuses)
}

/// Apply the update rule to the selected entries, then write their files and the lock together.
#[inline]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
pub fn update(
    root: &Path,
    kind: Kind,
    paths: &[PathBuf],
    reference: Option<&str>,
) -> Result<Vec<Updated>, AdoptionError> {
    let mut lock = read_lock(root)?;
    if lock.is_empty() {
        return Err(AdoptionError::NothingRecorded);
    }
    let units = select(&lock, kind, paths)?;
    let edits = units
        .iter()
        .map(|unit| unit.ensure_updatable(root))
        .collect::<Result<Vec<_>, _>>()?;
    let mut tracked = vec![PathBuf::from(LOCK_FILE)];
    tracked.extend(units.iter().flat_map(Unit::list_locked_paths));
    if has_uncommitted(root, kind, &tracked)? {
        return Err(AdoptionError::UncommittedUpdate(kind));
    }

    let mut libraries = Libraries::new(&lock);
    let mut writes = Vec::new();
    let mut recorded = Vec::new();
    let mut results = Vec::new();
    for (mut unit, edited) in units.into_iter().zip(edits) {
        let upstream = libraries.read_upstream(&unit.source)?;
        // Without a reference, stop at the next major, so each one arrives as its own update.
        let revision = match reference {
            Some(requested) => upstream.resolve(Some(requested))?,
            None => {
                match upstream.list_majors(&unit.reference)?.first() {
                    Some(next) => upstream.resolve(Some(next))?,
                    None => upstream.find_newest(&unit.reference)?,
                }
            },
        };
        let name = unit.derive_local_name(root)?;
        let Some(next) = upstream.read_folder(&revision.commit, kind, unit.origin.name())? else {
            results.push(Updated {
                path: unit.key,
                from: unit.reference,
                to: revision.reference,
                outcome: Outcome::Removed,
                files: BTreeMap::new(),
            });
            continue;
        };
        let planned = unit.plan_update(root, &name, &edited, upstream, &revision, next)?;
        let (outcome, mut files) = unit.split(planned.outcomes)?;
        files.retain(|_, file_outcome| *file_outcome != Outcome::Unchanged);
        writes.extend(planned.writes);
        recorded.extend(planned.files.keys().map(|path| unit.folder.join(path)));
        results.push(Updated {
            path: unit.key.clone(),
            from: unit.reference.clone(),
            to: revision.reference.clone(),
            outcome,
            files,
        });
        unit.reference = revision.reference;
        unit.commit = revision.commit;
        unit.files = planned.files;
        unit.store(&mut lock)?;
    }

    refuse_ignored(root, &recorded)?;
    for (path, file) in &writes {
        unit::write(root, path, file)?;
    }
    write_lock(root, &lock)?;
    Ok(results)
}

/// Return base, newest upstream, and local texts for one entry. Works while conflict markers
/// remain.
#[inline]
pub fn diff(root: &Path, kind: Kind, path: &Path) -> Result<Comparison, AdoptionError> {
    let lock = read_lock(root)?;
    let unit = Unit::find(&lock, kind, &kind.derive_key(path)?)?;
    let document = unit.name_document();
    let local = read_text(root, &document)?;
    let name = name_of(&local)?;
    let mut libraries = Libraries::new(&lock);
    let upstream = libraries.read_upstream(&unit.source)?;
    let base = upstream
        .read_folder(&unit.commit, kind, unit.origin.name())?
        .ok_or_else(|| unit.report_missing_at_lock())?;
    let revision = upstream.find_newest(&unit.reference)?;
    let next = upstream.read_folder(&revision.commit, kind, unit.origin.name())?;
    let renamed = |files: &Files| -> Result<String, AdoptionError> {
        let file = files
            .get(Path::new(kind.name_document()))
            .ok_or(AdoptionError::NoLibraryDocument(kind.name_document()))?;
        Ok(with_name(unit::decode_text(&file.bytes, &document)?, name)?)
    };
    let files = unit.compare(root, &base, next.as_ref())?;
    Ok(Comparison {
        path: unit.key,
        base: Version {
            reference: unit.reference.clone(),
            commit: unit.commit.clone(),
            text: renamed(&base)?,
        },
        upstream: next
            .as_ref()
            .map(|next_files| {
                Ok::<_, AdoptionError>(Version {
                    reference: revision.reference.clone(),
                    commit: revision.commit.clone(),
                    text: renamed(next_files)?,
                })
            })
            .transpose()?,
        local: local.clone(),
        files,
    })
}

/// Drop a lock entry. Unless the files are kept as local files, delete the locked ones and the
/// folders they leave empty, refusing while Git has uncommitted changes to any of them.
#[inline]
pub fn remove(
    root: &Path,
    kind: Kind,
    path: &Path,
    keep_file: bool,
) -> Result<PathBuf, AdoptionError> {
    let mut lock = read_lock(root)?;
    let key = kind.derive_key(path)?;
    let unit = Unit::find(&lock, kind, &key)?;
    if !keep_file && has_uncommitted(root, kind, &unit.list_locked_paths().collect::<Vec<_>>())? {
        return Err(AdoptionError::UncommittedRemove(key));
    }
    match kind {
        Kind::Contracts => {
            drop(lock.contracts.remove(&key));
        },
        Kind::Skills => {
            drop(lock.skills.remove(&key));
        },
    }
    if !keep_file {
        unit.delete(root)?;
    }
    write_lock(root, &lock)?;
    Ok(key)
}

/// Check the lock's format, that every locked file exists, that read-only files match their
/// hashes, and that installed skills' Markdown is valid. Works offline.
#[inline]
pub fn validate_lock(root: &Path) -> Result<Recorded, AdoptionError> {
    let lock = read_lock(root)?;
    let units = Unit::list_all(&lock, Kind::Contracts)?
        .into_iter()
        .chain(Unit::list_all(&lock, Kind::Skills)?);
    for unit in units {
        let kind = unit.read_kind();
        for path in unit.check_files(root)? {
            let file = unit.folder.join(&path);
            if kind.derive_rule(&path) == Rule::Merged
                && let Some(line) = find_conflict_marker(&read_text(root, &file)?)
            {
                return Err(AdoptionError::ConflictMarker { file, line });
            }
        }
        if kind == Kind::Skills {
            let document = unit.name_document();
            validate_skill(&document, &read_text(root, &document)?)?;
        }
    }
    Ok(Recorded {
        contracts: lock.contracts.len(),
        skills: lock.skills.len(),
    })
}

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

pub(crate) fn read_lock(root: &Path) -> Result<Lock, LockFileError> {
    let path = root.join(LOCK_FILE);
    if !path.exists() {
        return Ok(Lock::default());
    }
    let text = fs::read_to_string(path).map_err(LockFileError::Read)?;
    Ok(Lock::parse(&text)?)
}

/// Write the lock, or delete it once it records nothing. The lock is renamed into place, so an
/// interrupted write leaves the previous lock whole.
fn write_lock(root: &Path, lock: &Lock) -> Result<(), AdoptionError> {
    let path = root.join(LOCK_FILE);
    if lock.is_empty() {
        if path.exists() {
            fs::remove_file(path).map_err(AdoptionError::WriteLock)?;
        }
        return Ok(());
    }
    let mut file = NamedTempFile::new_in(root).map_err(AdoptionError::WriteLock)?;
    file.write_all(lock.render()?.as_bytes())
        .map_err(AdoptionError::WriteLock)?;
    // A temporary file starts readable only by its owner; the lock stays readable by everyone.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o644))
            .map_err(AdoptionError::WriteLock)?;
    }
    drop(file.persist(path).map_err(AdoptionError::PersistLock)?);
    Ok(())
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
