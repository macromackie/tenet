//! `add`: install a library contract or skill, or record an existing copy, and lock its files.

use alloc::collections::BTreeMap;
use std::{
    fs,
    path::{Path, PathBuf},
};

use tenet_contracts::{Lock, hash_file, is_kebab, is_skill_name, name_of, with_name};

use super::{
    Added,
    AdoptionError,
    Kind,
    Origin,
    kind::Rule,
    make_relative,
    parse_spec,
    read_lock,
    refuse_ignored,
    unit::{self, Unit},
    upstream::{File, Files, Revision, Upstream},
    write_lock,
};
use crate::discovery;

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
            let writes = fetched.rename_unit(
                root,
                kind,
                path.parent().ok_or(AdoptionError::NoContractFolder)?,
                chosen,
            )?;
            (path, chosen.to_owned(), writes)
        },
        Destination::Existing(path) => {
            let key = kind.derive_key(path)?;
            let local = existing
                .iter()
                .find(|entry| entry.path == key)
                .ok_or_else(|| AdoptionError::NotProjectContract(key.clone()))?;
            let folder = key.parent().ok_or(AdoptionError::NoContractFolder)?;
            let writes = fetched.find_missing(root, kind, folder, &local.name)?;
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
            let writes = fetched.rename_unit(root, kind, &folder, chosen)?;
            (folder, writes)
        },
        SkillDestination::Existing(path) => {
            let folder = kind.derive_key(path)?;
            let document = folder.join(kind.name_document());
            let local = fs::read_to_string(root.join(&document)).map_err(|source| {
                AdoptionError::ReadExistingSkill {
                    path: document.clone(),
                    source,
                }
            })?;
            kind.validate(&document, &local)?;
            let writes = fetched.find_missing(root, kind, &folder, name_of(&local)?)?;
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

/// Register `--source ALIAS=URL` values, then fetch `SOURCE/NAME[@REF]`.
fn read_request(
    lock: &mut Lock,
    kind: Kind,
    spec: &str,
    sources: &[String],
) -> Result<Fetched, AdoptionError> {
    let (source, name, reference) = parse_spec(spec)?;
    for registration in sources {
        let (alias, url) = registration
            .split_once('=')
            .ok_or(AdoptionError::SourceFormat)?;
        if !is_kebab(alias) {
            return Err(AdoptionError::SourceAlias);
        }
        if url.is_empty() || url.starts_with('-') {
            return Err(AdoptionError::SourceUrl(alias.to_owned()));
        }
        // `tenet` always names the public library, even before a lock exists.
        if let Some(existing) = lock.sources.get(alias)
            && existing != url
        {
            return Err(AdoptionError::SourceTaken {
                alias: alias.to_owned(),
                existing: existing.clone(),
                url: url.to_owned(),
                name: name.to_owned(),
            });
        }
        drop(lock.sources.insert(alias.into(), url.into()));
    }
    let url = lock
        .sources
        .get(source)
        .ok_or_else(|| AdoptionError::UnknownSource(source.to_owned()))?;
    let upstream = Upstream::download(url)?;
    let revision = upstream.resolve(reference)?;
    let files = upstream
        .read_folder(&revision.commit, kind, name)?
        .ok_or_else(|| {
            AdoptionError::NotInLibrary {
                library: [source, name].join("/"),
                reference: revision.reference.clone(),
            }
        })?;
    Ok(Fetched {
        files,
        name: name.into(),
        revision,
        source: source.into(),
    })
}

impl Fetched {
    /// The lock entry for a copy of these files at `key`.
    fn build_unit(&self, kind: Kind, key: PathBuf, folder: PathBuf) -> Result<Unit, AdoptionError> {
        let files = self
            .files
            .iter()
            .map(|(path, file)| Ok((path.clone(), kind.hash(path, &file.bytes)?)))
            .collect::<Result<BTreeMap<_, _>, AdoptionError>>()?;
        Ok(Unit {
            commit: self.revision.commit.clone(),
            files,
            folder,
            key,
            origin: match kind {
                Kind::Contracts => Origin::Contract(self.name.clone()),
                Kind::Skills => Origin::Skill(self.name.clone()),
            },
            reference: self.revision.reference.clone(),
            source: self.source.clone(),
        })
    }

    /// The files an existing copy in `folder`, whose local name is `name`, lacks. Refuses an
    /// upstream CONTRACT.md or SKILL.md this Tenet cannot read, a read-only file whose content
    /// differs from upstream, and a file in the way of a missing one. Leaves Markdown edits and
    /// files upstream does not have alone.
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    fn find_missing(
        &self,
        root: &Path,
        kind: Kind,
        folder: &Path,
        name: &str,
    ) -> Result<Vec<(PathBuf, File)>, AdoptionError> {
        let mut writes = Vec::new();
        for (path, file) in &self.files {
            let target = folder.join(path);
            let rule = kind.derive_rule(path);
            if rule == Rule::Named {
                drop(self.read_document(kind, &target, file, name)?);
            }
            match unit::find_obstruction(root, folder, path)? {
                None => writes.push((target, file.clone())),
                Some(found) if found == target => {
                    if rule == Rule::ReadOnly
                        && hash_file(&unit::read(root, &target)?) != hash_file(&file.bytes)
                    {
                        return Err(AdoptionError::ReadOnlyDiffers {
                            target,
                            release: self.release(),
                        });
                    }
                },
                Some(found) => {
                    return Err(AdoptionError::InTheWayOfAdd {
                        found,
                        target,
                        release: self.release(),
                    });
                },
            }
        }
        Ok(writes)
    }

    /// The upstream CONTRACT.md or SKILL.md renamed to `name`, refused unless this Tenet reads it
    /// at `target`.
    fn read_document(
        &self,
        kind: Kind,
        target: &Path,
        file: &File,
        name: &str,
    ) -> Result<File, AdoptionError> {
        let document = with_name(unit::decode_text(&file.bytes, target)?, name)?;
        // Report the upstream text's error as a message, since it does not point into a local file.
        kind.validate(target, &document).map_err(|error| {
            AdoptionError::CannotInstall {
                release: self.release(),
                error: Box::new(error),
            }
        })?;
        Ok(File {
            bytes: document.into_bytes(),
            executable: file.executable,
        })
    }

    /// The library entry and release, as `SOURCE/NAME REF`.
    fn release(&self) -> String {
        format!("{}/{} {}", self.source, self.name, self.revision.reference)
    }

    /// Every file for a new copy in `folder`, its CONTRACT.md or SKILL.md renamed to `name`.
    /// Refuses a path that already exists or a file in the way of one.
    fn rename_unit(
        &self,
        root: &Path,
        kind: Kind,
        folder: &Path,
        name: &str,
    ) -> Result<Vec<(PathBuf, File)>, AdoptionError> {
        let mut writes = Vec::new();
        for (path, file) in &self.files {
            let target = folder.join(path);
            if let Some(found) = unit::find_obstruction(root, folder, path)? {
                return Err(AdoptionError::Exists(found));
            }
            let written = match kind.derive_rule(path) {
                Rule::Named => self.read_document(kind, &target, file, name)?,
                Rule::Merged | Rule::ReadOnly => file.clone(),
            };
            writes.push((target, written));
        }
        Ok(writes)
    }
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
        unit::write(root, path, file)?;
    }
    unit.store(lock)?;
    write_lock(root, lock)
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
