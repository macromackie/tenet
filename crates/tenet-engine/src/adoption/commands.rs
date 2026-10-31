//! Installed-copy inspection and coordinated lock updates.

use alloc::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tenet_contracts::{LOCK_FILE, find_conflict_marker, name_of, validate_skill, with_name};

use super::{
    AdoptionError,
    Change,
    Comparison,
    Files,
    Kind,
    Libraries,
    LocalState,
    Newest,
    Outcome,
    Recorded,
    Rule,
    Status,
    Unit,
    Updated,
    Version,
    decode_text,
    has_uncommitted,
    read_text,
    refuse_ignored,
    select,
    write,
};
use crate::lock::{read_lock, write_lock};

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
        write(root, path, file)?;
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
        Ok(with_name(decode_text(&file.bytes, &document)?, name)?)
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
