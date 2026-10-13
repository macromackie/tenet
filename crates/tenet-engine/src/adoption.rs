//! Installing shared contracts and taking their upstream changes through `.tenet.lock`.
//!
//! Checking never reads the lock. Only these operations touch the network, and nothing persists outside the project.

mod upstream;

use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use tenet_contracts::{Entry, LOCK_FILE, Lock, content_hash, is_kebab, name_of, with_name};

use crate::{discovery, git};
use upstream::Upstream;

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

#[derive(Serialize)]
pub struct Added {
    pub path: PathBuf,
    pub name: String,
    pub source: String,
    pub contract: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub commit: String,
    pub written: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalState {
    Unchanged,
    Edited,
    Missing,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Current,
    Changed,
    Removed,
}

#[derive(Serialize)]
pub struct Newest {
    #[serde(rename = "ref")]
    pub reference: String,
    pub change: Change,
}

#[derive(Serialize)]
pub struct Status {
    pub path: PathBuf,
    /// The local name; `None` when the file is missing.
    pub name: Option<String>,
    pub source: String,
    pub contract: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub state: LocalState,
    /// `None` when checked offline.
    pub newest: Option<Newest>,
}

impl Status {
    /// True when the copy is missing or upstream changed or removed it.
    pub fn needs_attention(&self) -> bool {
        self.state == LocalState::Missing
            || self
                .newest
                .as_ref()
                .is_some_and(|newest| newest.change != Change::Current)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    Unchanged,
    Replaced,
    Merged,
    Conflict { regions: u32 },
    Removed,
}

#[derive(Serialize)]
pub struct Updated {
    pub path: PathBuf,
    pub from: String,
    pub to: String,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Serialize)]
pub struct Version {
    #[serde(rename = "ref")]
    pub reference: String,
    pub commit: String,
    pub text: String,
}

/// The three texts behind an update, each rendered with the local name.
#[derive(Serialize)]
pub struct Comparison {
    pub path: PathBuf,
    pub base: Version,
    /// `None` when upstream removed the contract.
    pub upstream: Option<Version>,
    pub local: String,
}

pub fn add(root: &Path, request: &AddRequest<'_>) -> Result<Added> {
    let (source, contract, reference) = parse_spec(request.spec)?;
    let mut lock = read_lock(root)?;
    for registration in request.sources {
        let (alias, url) = registration
            .split_once('=')
            .context("--source expects ALIAS=URL")?;
        ensure!(is_kebab(alias), "source alias must be lowercase kebab-case");
        ensure!(
            !url.is_empty() && !url.starts_with('-'),
            "invalid URL for source {alias}"
        );
        if let Some(existing) = lock.sources.get(alias) {
            ensure!(
                existing == url,
                "source {alias} is already {existing} in {LOCK_FILE}"
            );
        }
        lock.sources.insert(alias.into(), url.into());
    }
    let url = lock.sources.get(source).with_context(|| {
        format!("unknown source {source}; register it with --source {source}=URL")
    })?;
    let upstream = Upstream::clone(url)?;
    let revision = upstream.resolve(reference)?;
    let text = upstream
        .contract(&revision.commit, contract)?
        .with_context(|| {
            format!(
                "{source}/{contract} does not exist at {}",
                revision.reference
            )
        })?;
    let existing = discovery::discover_contracts(root)?;

    let (path, name, document) = match request.destination {
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
            let name = name.unwrap_or(contract);
            ensure!(
                existing.iter().all(|c| c.name != name),
                "contract name {name} is already used; choose another with --name {}-{contract}",
                scope
                    .file_name()
                    .map_or("local".into(), |n| n.to_string_lossy())
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
                .join("CONTRACT.md");
            ensure!(
                !root.join(&path).exists(),
                "{} already exists",
                path.display()
            );
            let document = with_name(&text, name)?;
            tenet_contracts::parse(&path, &document)?;
            (path, name.to_owned(), Some(document))
        }
        Destination::Existing(path) => {
            let path = key(path)?;
            let local = existing
                .iter()
                .find(|c| c.path == path)
                .with_context(|| format!("{} is not a contract in this project", path.display()))?;
            (path, local.name.clone(), None)
        }
    };
    ensure!(
        !lock.contracts.contains_key(&path),
        "{} is already recorded in {LOCK_FILE}",
        path.display()
    );

    let written = document.is_some();
    if let Some(document) = document {
        let full = root.join(&path);
        fs::create_dir_all(full.parent().context("contract has no folder")?)?;
        fs::write(&full, document)?;
    }
    lock.contracts.insert(
        path.clone(),
        Entry {
            source: source.into(),
            contract: contract.into(),
            reference: revision.reference.clone(),
            commit: revision.commit.clone(),
            hash: content_hash(&text)?,
        },
    );
    write_lock(root, &lock)?;
    Ok(Added {
        path,
        name,
        source: source.into(),
        contract: contract.into(),
        reference: revision.reference,
        commit: revision.commit,
        written,
    })
}

pub fn outdated(root: &Path, offline: bool) -> Result<Vec<Status>> {
    let lock = read_lock(root)?;
    let mut libraries = Libraries::new(&lock);
    let mut statuses = Vec::new();
    for (path, entry) in &lock.contracts {
        let full = root.join(path);
        let (state, name) = if full.exists() {
            let text = fs::read_to_string(&full)?;
            let state = if content_hash(&text)? == entry.hash {
                LocalState::Unchanged
            } else {
                LocalState::Edited
            };
            (state, Some(name_of(&text)?.to_owned()))
        } else {
            (LocalState::Missing, None)
        };
        let newest = if offline {
            None
        } else {
            let upstream = libraries.get(&entry.source)?;
            let revision = upstream.newest(&entry.reference)?;
            let change = match upstream.contract(&revision.commit, &entry.contract)? {
                None => Change::Removed,
                Some(text) if content_hash(&text)? == entry.hash => Change::Current,
                Some(_) => Change::Changed,
            };
            Some(Newest {
                reference: revision.reference,
                change,
            })
        };
        statuses.push(Status {
            path: path.clone(),
            name,
            source: entry.source.clone(),
            contract: entry.contract.clone(),
            reference: entry.reference.clone(),
            state,
            newest,
        });
    }
    Ok(statuses)
}

/// Apply the update rule to the selected entries, then write their files and the lock together.
pub fn update(root: &Path, paths: &[PathBuf], reference: Option<&str>) -> Result<Vec<Updated>> {
    let mut lock = read_lock(root)?;
    ensure!(
        !lock.contracts.is_empty(),
        "no contracts are recorded in {LOCK_FILE}"
    );
    let selected = select(&lock, paths)?;
    for path in &selected {
        ensure!(
            root.join(path).is_file(),
            "{0} is missing; restore it, fix its path in {LOCK_FILE}, or run `tenet contracts remove {0}`",
            path.display()
        );
    }
    let status = git::output(
        root,
        &[
            &["status", "--porcelain", "--", LOCK_FILE][..],
            &selected
                .iter()
                .map(|p| p.to_str().context("path is not UTF-8"))
                .collect::<Result<Vec<_>>>()?,
        ]
        .concat(),
    )
    .context("tenet contracts update requires a Git checkout")?;
    ensure!(
        status.is_empty(),
        "commit or set aside changes to {LOCK_FILE} and the selected contracts first"
    );

    let mut libraries = Libraries::new(&lock);
    let mut writes = Vec::new();
    let mut results = Vec::new();
    for path in selected {
        let entry = lock.contracts[&path].clone();
        let upstream = libraries.get(&entry.source)?;
        let revision = match reference {
            Some(reference) => upstream.resolve(Some(reference))?,
            None => upstream.newest(&entry.reference)?,
        };
        let local = fs::read_to_string(root.join(&path))?;
        let name = tenet_contracts::parse(&path, &local)?.name;
        let Some(next) = upstream.contract(&revision.commit, &entry.contract)? else {
            results.push(Updated {
                path,
                from: entry.reference.clone(),
                to: revision.reference,
                outcome: Outcome::Removed,
            });
            continue;
        };
        let next_hash = content_hash(&next)?;
        let next = with_name(&next, &name)?;
        tenet_contracts::parse(&path, &next)
            .with_context(|| format!("{} needs a newer Tenet", revision.reference))?;

        let outcome = if next_hash == entry.hash {
            Outcome::Unchanged
        } else if content_hash(&local)? == entry.hash {
            writes.push((path.clone(), next));
            Outcome::Replaced
        } else {
            let base = upstream
                .contract(&entry.commit, &entry.contract)?
                .with_context(|| {
                    format!(
                        "the locked commit {} has no {}",
                        entry.commit, entry.contract
                    )
                })?;
            let merge = upstream::merge(
                &local,
                &with_name(&base, &name)?,
                &next,
                [
                    "local",
                    &format!("base {}", entry.reference),
                    &format!("upstream {}", revision.reference),
                ],
            )?;
            writes.push((path.clone(), merge.text));
            if merge.conflicts == 0 {
                Outcome::Merged
            } else {
                Outcome::Conflict {
                    regions: merge.conflicts,
                }
            }
        };
        results.push(Updated {
            path: path.clone(),
            from: entry.reference.clone(),
            to: revision.reference.clone(),
            outcome,
        });
        lock.contracts.insert(
            path,
            Entry {
                reference: revision.reference,
                commit: revision.commit,
                hash: next_hash,
                ..entry
            },
        );
    }

    for (path, text) in writes {
        fs::write(root.join(path), text)?;
    }
    write_lock(root, &lock)?;
    Ok(results)
}

/// Return base, newest upstream, and local text for one entry. Works while conflict markers remain.
pub fn diff(root: &Path, path: &Path) -> Result<Comparison> {
    let lock = read_lock(root)?;
    let path = key(path)?;
    let entry = lock
        .contracts
        .get(&path)
        .with_context(|| format!("{} is not recorded in {LOCK_FILE}", path.display()))?;
    let local = fs::read_to_string(root.join(&path))
        .with_context(|| format!("cannot read {}", path.display()))?;
    let name = name_of(&local)?;
    let mut libraries = Libraries::new(&lock);
    let upstream = libraries.get(&entry.source)?;
    let base = upstream
        .contract(&entry.commit, &entry.contract)?
        .with_context(|| {
            format!(
                "the locked commit {} has no {}",
                entry.commit, entry.contract
            )
        })?;
    let revision = upstream.newest(&entry.reference)?;
    let newest = upstream
        .contract(&revision.commit, &entry.contract)?
        .map(|text| with_name(&text, name))
        .transpose()?;
    Ok(Comparison {
        base: Version {
            reference: entry.reference.clone(),
            commit: entry.commit.clone(),
            text: with_name(&base, name)?,
        },
        upstream: newest.map(|text| Version {
            reference: revision.reference,
            commit: revision.commit,
            text,
        }),
        local: local.clone(),
        path,
    })
}

/// Drop a lock entry. Unless the file is kept as a local contract, delete it and its folder once empty.
pub fn remove(root: &Path, path: &Path, keep_file: bool) -> Result<PathBuf> {
    let mut lock = read_lock(root)?;
    let path = key(path)?;
    ensure!(
        lock.contracts.remove(&path).is_some(),
        "{} is not recorded in {LOCK_FILE}",
        path.display()
    );
    if !keep_file {
        let full = root.join(&path);
        if full.exists() {
            fs::remove_file(&full)?;
        }
        let folder = full.parent().context("contract has no folder")?;
        if folder.exists() && folder.read_dir()?.next().is_none() {
            fs::remove_dir(folder)?;
        }
    }
    write_lock(root, &lock)?;
    Ok(path)
}

/// Check the lock's format and that every entry's file exists. Returns the number of entries.
pub fn validate_lock(root: &Path) -> Result<usize> {
    let path = root.join(LOCK_FILE);
    if !path.exists() {
        return Ok(0);
    }
    let lock = Lock::parse(&fs::read_to_string(&path)?)?;
    for entry in lock.contracts.keys() {
        ensure!(
            root.join(entry).is_file(),
            "{LOCK_FILE} records {}, which does not exist",
            entry.display()
        );
    }
    Ok(lock.contracts.len())
}

/// Clones each source at most once per command.
struct Libraries {
    sources: BTreeMap<String, String>,
    clones: BTreeMap<String, Upstream>,
}

impl Libraries {
    fn new(lock: &Lock) -> Self {
        Self {
            sources: lock.sources.clone(),
            clones: BTreeMap::new(),
        }
    }

    fn get(&mut self, source: &str) -> Result<&Upstream> {
        if !self.clones.contains_key(source) {
            let url = self
                .sources
                .get(source)
                .with_context(|| format!("{LOCK_FILE} names unknown source {source}"))?;
            self.clones.insert(source.into(), Upstream::clone(url)?);
        }
        self.clones.get(source).context("source clone disappeared")
    }
}

fn read_lock(root: &Path) -> Result<Lock> {
    let path = root.join(LOCK_FILE);
    if !path.exists() {
        return Ok(Lock::default());
    }
    Lock::parse(&fs::read_to_string(&path)?)
}

fn write_lock(root: &Path, lock: &Lock) -> Result<()> {
    let path = root.join(LOCK_FILE);
    if lock.contracts.is_empty() {
        if path.exists() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }
    fs::write(path, lock.render()?)?;
    Ok(())
}

fn parse_spec(spec: &str) -> Result<(&str, &str, Option<&str>)> {
    let (source, rest) = spec
        .split_once('/')
        .context("expected SOURCE/NAME[@REF], such as tenet/data-and-apis")?;
    let (name, reference) = match rest.split_once('@') {
        Some((name, reference)) => (name, Some(reference)),
        None => (rest, None),
    };
    ensure!(
        is_kebab(source) && is_kebab(name),
        "expected SOURCE/NAME[@REF] with kebab-case names"
    );
    Ok((source, name, reference))
}

fn select(lock: &Lock, paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    if paths.is_empty() {
        return Ok(lock.contracts.keys().cloned().collect());
    }
    paths
        .iter()
        .map(|path| {
            let path = key(path)?;
            ensure!(
                lock.contracts.contains_key(&path),
                "{} is not recorded in {LOCK_FILE}",
                path.display()
            );
            Ok(path)
        })
        .collect()
}

/// Accept a contract folder or its `CONTRACT.md`, relative to the root.
fn key(path: &Path) -> Result<PathBuf> {
    let path = relative(path)?;
    Ok(
        if path.file_name().is_some_and(|name| name == "CONTRACT.md") {
            path
        } else {
            path.join("CONTRACT.md")
        },
    )
}

fn relative(path: &Path) -> Result<PathBuf> {
    let mut normal = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(part) => normal.push(part),
            Component::CurDir => {}
            _ => bail!("{} must be relative to the project root", path.display()),
        }
    }
    Ok(normal)
}
