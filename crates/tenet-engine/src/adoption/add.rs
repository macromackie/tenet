//! `add`: install a library contract or skill, or record an existing copy, and lock its files.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use tenet_contracts::{LOCK_FILE, Lock, file_hash, is_kebab, is_skill_name, name_of, with_name};

use super::{
    Added, Kind, Origin,
    kind::Rule,
    parse_spec, read_lock, refuse_ignored, relative,
    unit::{self, Unit},
    upstream::{File, Files, Revision, Upstream},
    write_lock,
};
use crate::discovery;

/// Where `add` puts a contract: a new numbered folder in a scope, or an existing copy recorded as is.
pub enum Destination<'a> {
    Scope {
        scope: &'a Path,
        name: Option<&'a str>,
        number: Option<u32>,
    },
    Existing(&'a Path),
}

pub struct AddRequest<'a> {
    /// `SOURCE/NAME[@REF]`.
    pub spec: &'a str,
    pub destination: Destination<'a>,
    /// `ALIAS=URL` registrations for non-default sources.
    pub sources: &'a [String],
}

/// Where `add_skill` puts a skill: a new folder in a directory, or an existing copy recorded as is.
pub enum SkillDestination<'a> {
    Directory {
        directory: &'a Path,
        name: Option<&'a str>,
    },
    Existing(&'a Path),
}

pub struct SkillRequest<'a> {
    /// `SOURCE/NAME[@REF]`.
    pub spec: &'a str,
    pub destination: SkillDestination<'a>,
    /// `ALIAS=URL` registrations for non-default sources.
    pub sources: &'a [String],
}

/// A library entry's files at the revision an `add` resolved.
struct Fetched {
    source: String,
    name: String,
    revision: Revision,
    files: Files,
}

pub fn add(root: &Path, request: &AddRequest<'_>) -> Result<Added> {
    let kind = Kind::Contracts;
    let mut lock = read_lock(root)?;
    let fetched = fetch(&mut lock, kind, request.spec, request.sources)?;
    let existing = discovery::discover_contracts(root)?;

    let (path, name, writes) = match request.destination {
        Destination::Scope {
            scope,
            name,
            number,
        } => {
            let scope = relative(scope)?;
            ensure!(
                root.join(&scope).is_dir(),
                "scope {} is not a directory",
                scope.display()
            );
            let name = name.unwrap_or(&fetched.name);
            ensure!(
                existing.iter().all(|c| c.name != name),
                "contract name {name} is already used; choose another with --name {}-{}",
                scope
                    .file_name()
                    .map_or("local".into(), |n| n.to_string_lossy()),
                fetched.name
            );
            let in_scope = existing.iter().filter(|c| c.scope == scope);
            let number = match number {
                Some(number) => number,
                None => in_scope.clone().map(|c| c.number).max().unwrap_or(0) + 1,
            };
            ensure!(
                in_scope.clone().all(|c| c.number != number),
                "contract number {number:03} is already used in {}",
                scope.display()
            );
            let path = scope
                .join(".contracts")
                .join(format!("{number:03}-{name}"))
                .join(kind.document());
            let writes = fetched.renamed(
                root,
                kind,
                path.parent().context("contract has no folder")?,
                name,
            )?;
            (path, name.to_owned(), writes)
        }
        Destination::Existing(path) => {
            let path = kind.key(path)?;
            let local = existing
                .iter()
                .find(|c| c.path == path)
                .with_context(|| format!("{} is not a contract in this project", path.display()))?;
            let folder = path.parent().context("contract has no folder")?;
            let writes = fetched.missing(root, kind, folder, &local.name)?;
            (path, local.name.clone(), writes)
        }
    };
    let folder = path.parent().context("contract has no folder")?.to_owned();
    let unit = fetched.unit(kind, path, folder)?;
    let added = added(
        &unit,
        name,
        matches!(request.destination, Destination::Scope { .. }),
    );
    record(root, &mut lock, unit, &writes)?;
    Ok(added)
}

pub fn add_skill(root: &Path, request: &SkillRequest<'_>) -> Result<Added> {
    let kind = Kind::Skills;
    if let SkillDestination::Directory {
        name: Some(name), ..
    } = request.destination
    {
        ensure!(
            is_skill_name(name),
            "skill name {name} must use lowercase letters, digits, and single hyphens, at most 64 characters"
        );
    }
    let mut lock = read_lock(root)?;
    let fetched = fetch(&mut lock, kind, request.spec, request.sources)?;

    let (folder, writes) = match request.destination {
        SkillDestination::Directory { directory, name } => {
            let name = name.unwrap_or(&fetched.name);
            let folder = relative(directory)?.join(name);
            ensure!(
                root.join(&folder).symlink_metadata().is_err(),
                "{0} already exists; record it with --path {0}",
                folder.display()
            );
            let writes = fetched.renamed(root, kind, &folder, name)?;
            (folder, writes)
        }
        SkillDestination::Existing(path) => {
            let folder = kind.key(path)?;
            let document = folder.join(kind.document());
            let local = fs::read_to_string(root.join(&document)).with_context(|| {
                format!(
                    "cannot read {}; install a new copy with --dir",
                    document.display()
                )
            })?;
            kind.validate(&document, &local)?;
            let writes = fetched.missing(root, kind, &folder, name_of(&local)?)?;
            (folder, writes)
        }
    };
    let name = folder
        .file_name()
        .context("skill has no folder name")?
        .to_string_lossy()
        .into_owned();
    let unit = fetched.unit(kind, folder.clone(), folder)?;
    let added = added(
        &unit,
        name,
        matches!(request.destination, SkillDestination::Directory { .. }),
    );
    record(root, &mut lock, unit, &writes)?;
    Ok(added)
}

/// Register `--source ALIAS=URL` values, then fetch `SOURCE/NAME[@REF]`.
fn fetch(lock: &mut Lock, kind: Kind, spec: &str, sources: &[String]) -> Result<Fetched> {
    let (source, name, reference) = parse_spec(spec)?;
    for registration in sources {
        let (alias, url) = registration
            .split_once('=')
            .context("--source expects ALIAS=URL")?;
        ensure!(is_kebab(alias), "source alias must be lowercase kebab-case");
        ensure!(
            !url.is_empty() && !url.starts_with('-'),
            "invalid URL for source {alias}"
        );
        // `tenet` always names the public library, even before a lock exists.
        if let Some(existing) = lock.sources.get(alias) {
            ensure!(
                existing == url,
                "source {alias} already names {existing}; register {url} under another alias, \
                 as in --source local={url} with local/{name}"
            );
        }
        lock.sources.insert(alias.into(), url.into());
    }
    let url = lock.sources.get(source).with_context(|| {
        format!("unknown source {source}; register it with --source {source}=URL")
    })?;
    let upstream = Upstream::clone(url)?;
    let revision = upstream.resolve(reference)?;
    let files = upstream
        .folder(&revision.commit, kind, name)?
        .with_context(|| format!("{source}/{name} does not exist at {}", revision.reference))?;
    Ok(Fetched {
        source: source.into(),
        name: name.into(),
        revision,
        files,
    })
}

impl Fetched {
    /// The lock entry for a copy of these files at `key`.
    fn unit(&self, kind: Kind, key: PathBuf, folder: PathBuf) -> Result<Unit> {
        let files = self
            .files
            .iter()
            .map(|(path, file)| Ok((path.clone(), kind.hash(path, &file.bytes)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok(Unit {
            origin: match kind {
                Kind::Contracts => Origin::Contract(self.name.clone()),
                Kind::Skills => Origin::Skill(self.name.clone()),
            },
            key,
            folder,
            source: self.source.clone(),
            reference: self.revision.reference.clone(),
            commit: self.revision.commit.clone(),
            files,
        })
    }

    /// Every file for a new copy in `folder`, its CONTRACT.md or SKILL.md renamed to `name`.
    /// Refuses a path that already exists or a file in the way of one.
    fn renamed(
        &self,
        root: &Path,
        kind: Kind,
        folder: &Path,
        name: &str,
    ) -> Result<Vec<(PathBuf, File)>> {
        let mut writes = Vec::new();
        for (path, file) in &self.files {
            let target = folder.join(path);
            if let Some(found) = unit::obstruction(root, folder, path)? {
                bail!("{} already exists", found.display());
            }
            let file = match kind.rule(path) {
                Rule::Named => self.document(kind, &target, file, name)?,
                Rule::Merged | Rule::ReadOnly => file.clone(),
            };
            writes.push((target, file));
        }
        Ok(writes)
    }

    /// The files an existing copy in `folder`, whose local name is `name`, lacks. Refuses an
    /// upstream CONTRACT.md or SKILL.md this Tenet cannot read, a read-only file whose content
    /// differs from upstream, and a file in the way of a missing one. Leaves Markdown edits and
    /// files upstream does not have alone.
    fn missing(
        &self,
        root: &Path,
        kind: Kind,
        folder: &Path,
        name: &str,
    ) -> Result<Vec<(PathBuf, File)>> {
        let mut writes = Vec::new();
        for (path, file) in &self.files {
            let target = folder.join(path);
            let rule = kind.rule(path);
            if rule == Rule::Named {
                self.document(kind, &target, file, name)?;
            }
            match unit::obstruction(root, folder, path)? {
                None => writes.push((target, file.clone())),
                Some(found) if found == target => ensure!(
                    rule != Rule::ReadOnly
                        || file_hash(&unit::read(root, &target)?) == file_hash(&file.bytes),
                    "{} differs from {}/{} {}; Tenet installs it read-only, so restore the released file before recording this copy",
                    target.display(),
                    self.source,
                    self.name,
                    self.revision.reference
                ),
                Some(found) => bail!(
                    "{} is in the way of {}, which {}/{} {} has; move it aside and add again",
                    found.display(),
                    target.display(),
                    self.source,
                    self.name,
                    self.revision.reference
                ),
            }
        }
        Ok(writes)
    }

    /// The upstream CONTRACT.md or SKILL.md renamed to `name`, refused unless this Tenet reads it
    /// at `target`.
    fn document(&self, kind: Kind, target: &Path, file: &File, name: &str) -> Result<File> {
        let document = with_name(unit::text(&file.bytes, target)?, name)?;
        // Report the upstream text's error as a message, since it does not point into a local file.
        kind.validate(target, &document).map_err(|error| {
            anyhow!(
                "cannot install {}/{} {}: {error:#}",
                self.source,
                self.name,
                self.revision.reference
            )
        })?;
        Ok(File {
            bytes: document.into_bytes(),
            executable: file.executable,
        })
    }
}

/// Write a new copy's files and record it, refusing a copy the lock already records or a file Git
/// ignores.
fn record(root: &Path, lock: &mut Lock, unit: Unit, writes: &[(PathBuf, File)]) -> Result<()> {
    let recorded = match unit.kind() {
        Kind::Contracts => lock.contracts.contains_key(&unit.key),
        Kind::Skills => lock.skills.contains_key(&unit.key),
    };
    ensure!(
        !recorded,
        "{} is already recorded in {LOCK_FILE}",
        unit.key.display()
    );
    refuse_ignored(root, &unit.locked_paths().collect::<Vec<_>>())?;
    for (path, file) in writes {
        unit::write(root, path, file)?;
    }
    unit.store(lock)?;
    write_lock(root, lock)
}

fn added(unit: &Unit, name: String, written: bool) -> Added {
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
