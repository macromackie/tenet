//! `add`: install a library contract or skill, or record an existing copy, and lock its files.

mod fetched;

use std::{
    fs,
    path::{Path, PathBuf},
};

use fetched::{Installation, InstallationMode, read_request};
use tenet_contracts::{Lock, is_skill_name, name_of};

use super::{
    Added,
    AdoptionError,
    File,
    Files,
    Kind,
    Origin,
    Revision,
    Rule,
    Unit,
    Upstream,
    decode_text,
    find_obstruction,
    make_relative,
    parse_spec,
    read,
    refuse_ignored,
    write,
};
use crate::{
    discovery,
    lock::{read_lock, write_lock},
};

/// Where `add` puts a contract: a new numbered folder in a scope, or an existing copy recorded as
/// is.
#[derive(Debug)]
pub enum Destination<'input> {
    Existing(&'input Path),
    Scope {
        scope: &'input Path,
        name: Option<&'input str>,
        number: Option<u32>,
    },
}

/// What `tenet contracts add` installs, and from where.
#[derive(Debug)]
pub struct AddRequest<'input> {
    pub destination: Destination<'input>,
    /// `ALIAS=URL` registrations for non-default sources.
    pub sources: &'input [String],
    /// `SOURCE/NAME[@REF]`.
    pub spec: &'input str,
}

/// Where `add_skill` puts a skill: a new folder in a directory, or an existing copy recorded as is.
#[derive(Debug)]
pub enum SkillDestination<'input> {
    Directory {
        directory: &'input Path,
        name: Option<&'input str>,
    },
    Existing(&'input Path),
}

/// What `tenet skills add` installs, and from where.
#[derive(Debug)]
pub struct SkillRequest<'input> {
    pub destination: SkillDestination<'input>,
    /// `ALIAS=URL` registrations for non-default sources.
    pub sources: &'input [String],
    /// `SOURCE/NAME[@REF]`.
    pub spec: &'input str,
}

/// A library entry's files at the revision an `add` resolved.
struct Fetched {
    files: Files,
    name: String,
    revision: Revision,
    source: String,
}

/// Install a library contract into the project and record it in the lock.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "The next contract number is one above the highest existing number in the scope, \
              which the scope's directory listing bounds."
)]
#[inline]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
pub fn add(root: &Path, request: &AddRequest<'_>) -> Result<Added, AdoptionError> {
    let kind = Kind::Contracts;
    let mut lock = read_lock(root)?;
    let fetched = read_request(&mut lock, kind, request.spec, request.sources)?;
    let existing = discovery::discover_contracts(root)?;

    let (path, name, writes) = match request.destination {
        Destination::Scope {
            scope,
            name,
            number,
        } => {
            let relative_scope = make_relative(scope)?;
            if !root.join(&relative_scope).is_dir() {
                return Err(AdoptionError::ScopeNotDirectory(relative_scope));
            }
            let chosen = name.unwrap_or(&fetched.name);
            if existing.iter().any(|entry| entry.name == chosen) {
                return Err(AdoptionError::NameUsed {
                    name: chosen.to_owned(),
                    scope: relative_scope
                        .file_name()
                        .map_or_else(|| "local".into(), |file_name| file_name.to_string_lossy())
                        .into_owned(),
                    library_name: fetched.name.clone(),
                });
            }
            let in_scope = existing
                .iter()
                .filter(|entry| entry.scope == relative_scope);
            let assigned = match number {
                Some(given) => given,
                None => {
                    in_scope
                        .clone()
                        .map(|entry| entry.number)
                        .max()
                        .unwrap_or(0)
                        + 1
                },
            };
            if in_scope.clone().any(|entry| entry.number == assigned) {
                return Err(AdoptionError::NumberUsed {
                    number: assigned,
                    scope: relative_scope,
                });
            }
            let path = relative_scope
                .join(".contracts")
                .join(format!("{assigned:03}-{chosen}"))
                .join(kind.name_document());
            let installation = Installation::new(
                root,
                kind,
                path.parent().ok_or(AdoptionError::NoContractFolder)?,
                chosen,
                InstallationMode::New,
            );
            let writes = fetched.prepare_files(&installation)?;
            (path, chosen.to_owned(), writes)
        },
        Destination::Existing(path) => {
            let key = kind.derive_key(path)?;
            let local = existing
                .iter()
                .find(|entry| entry.path == key)
                .ok_or_else(|| AdoptionError::NotProjectContract(key.clone()))?;
            let folder = key.parent().ok_or(AdoptionError::NoContractFolder)?;
            let installation =
                Installation::new(root, kind, folder, &local.name, InstallationMode::Existing);
            let writes = fetched.prepare_files(&installation)?;
            (key, local.name.clone(), writes)
        },
    };
    let folder = path
        .parent()
        .ok_or(AdoptionError::NoContractFolder)?
        .to_owned();
    let unit = fetched.build_unit(kind, path, folder)?;
    let added = build_added(
        &unit,
        name,
        matches!(request.destination, Destination::Scope { .. }),
    );
    record(root, &mut lock, unit, &writes)?;
    Ok(added)
}

/// Install a library skill into the project and record it in the lock.
#[inline]
pub fn add_skill(root: &Path, request: &SkillRequest<'_>) -> Result<Added, AdoptionError> {
    let kind = Kind::Skills;
    if let SkillDestination::Directory {
        name: Some(name), ..
    } = request.destination
        && !is_skill_name(name)
    {
        return Err(AdoptionError::SkillName(name.to_owned()));
    }
    let mut lock = read_lock(root)?;
    let fetched = read_request(&mut lock, kind, request.spec, request.sources)?;

    let (folder, writes) = match request.destination {
        SkillDestination::Directory { directory, name } => {
            let chosen = name.unwrap_or(&fetched.name);
            let folder = make_relative(directory)?.join(chosen);
            if root.join(&folder).symlink_metadata().is_ok() {
                return Err(AdoptionError::SkillExists(folder));
            }
            let installation =
                Installation::new(root, kind, &folder, chosen, InstallationMode::New);
            let writes = fetched.prepare_files(&installation)?;
            (folder, writes)
        },
        SkillDestination::Existing(path) => {
            let folder = kind.derive_key(path)?;
            let local = read_existing_skill(root, kind, &folder)?;
            let installation = Installation::new(
                root,
                kind,
                &folder,
                name_of(&local)?,
                InstallationMode::Existing,
            );
            let writes = fetched.prepare_files(&installation)?;
            (folder, writes)
        },
    };
    let name = folder
        .file_name()
        .ok_or(AdoptionError::NoSkillFolderName)?
        .to_string_lossy()
        .into_owned();
    let unit = fetched.build_unit(kind, folder.clone(), folder)?;
    let added = build_added(
        &unit,
        name,
        matches!(request.destination, SkillDestination::Directory { .. }),
    );
    record(root, &mut lock, unit, &writes)?;
    Ok(added)
}

fn read_existing_skill(root: &Path, kind: Kind, folder: &Path) -> Result<String, AdoptionError> {
    let document = folder.join(kind.name_document());
    let local = fs::read_to_string(root.join(&document)).map_err(|source| {
        AdoptionError::ReadExistingSkill {
            path: document.clone(),
            source,
        }
    })?;
    kind.validate(&document, &local)?;
    Ok(local)
}

/// Write a new copy's files and record it, refusing a copy the lock already records or a file Git
/// ignores.
fn record(
    root: &Path,
    lock: &mut Lock,
    unit: Unit,
    writes: &[(PathBuf, File)],
) -> Result<(), AdoptionError> {
    let recorded = match unit.read_kind() {
        Kind::Contracts => lock.contracts.contains_key(&unit.key),
        Kind::Skills => lock.skills.contains_key(&unit.key),
    };
    if recorded {
        return Err(AdoptionError::AlreadyRecorded(unit.key));
    }
    refuse_ignored(root, &unit.list_locked_paths().collect::<Vec<_>>())?;
    for (path, file) in writes {
        write(root, path, file)?;
    }
    unit.store(lock)?;
    write_lock(root, lock).map_err(AdoptionError::from)
}

fn build_added(unit: &Unit, name: String, written: bool) -> Added {
    Added {
        path: unit.key.clone(),
        name,
        source: unit.source.clone(),
        origin: unit.origin.clone(),
        reference: unit.reference.clone(),
        commit: unit.commit.clone(),
        written,
    }
}
