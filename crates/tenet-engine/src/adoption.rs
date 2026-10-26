//! Installing shared contracts and skills through `.tenet.lock`, and taking their upstream changes.
//!
//! Checking never reads the lock. Only these operations touch the network, and nothing persists outside the project.

mod add;
mod kind;
mod unit;
mod upstream;

use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use tenet_contracts::{LOCK_FILE, Lock, conflict_marker, name_of, validate_skill, with_name};

use crate::git;
pub use add::{AddRequest, Destination, SkillDestination, SkillRequest, add, add_skill};
pub use kind::Kind;
use kind::Rule;
use unit::Unit;
use upstream::{Files, Upstream};

/// The library entry an installed copy came from, written as its `contract` or `skill` name.
#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Contract(String),
    Skill(String),
}

impl Origin {
    pub fn kind(&self) -> Kind {
        match self {
            Origin::Contract(_) => Kind::Contracts,
            Origin::Skill(_) => Kind::Skills,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Origin::Contract(name) | Origin::Skill(name) => name,
        }
    }
}

#[derive(Serialize)]
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
    Added,
    Removed,
}

#[derive(Serialize)]
pub struct Newest {
    #[serde(rename = "ref")]
    pub reference: String,
    /// The CONTRACT.md or SKILL.md, or the whole copy when upstream removed it.
    pub change: Change,
    /// The other files upstream changed, added, or removed, relative to the copy's folder.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<PathBuf, Change>,
}

#[derive(Serialize)]
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
    pub fn needs_attention(&self) -> bool {
        let kind = self.origin.kind();
        self.state == LocalState::Missing
            || self.files.iter().any(|(path, state)| {
                *state == LocalState::Missing || kind.rule(path) == Rule::ReadOnly
            })
            || self
                .newest
                .as_ref()
                .is_some_and(|newest| newest.change != Change::Current || !newest.files.is_empty())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    Unchanged,
    Replaced,
    Merged,
    Conflict { regions: u32 },
    Added,
    Removed,
}

#[derive(Serialize)]
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
    pub fn needs_attention(&self) -> bool {
        std::iter::once(&self.outcome)
            .chain(self.files.values())
            .any(|outcome| matches!(outcome, Outcome::Conflict { .. } | Outcome::Removed))
    }
}

#[derive(Serialize)]
pub struct Version {
    #[serde(rename = "ref")]
    pub reference: String,
    pub commit: String,
    pub text: String,
}

/// The texts behind an update, the CONTRACT.md or SKILL.md rendered with the local name.
#[derive(Serialize)]
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

/// One other file of a copy at the locked release, the newest release, and locally. Bytes that are
/// not UTF-8 read as replacement characters.
#[derive(Serialize)]
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
pub struct Recorded {
    pub contracts: usize,
    pub skills: usize,
}

pub fn outdated(root: &Path, kind: Kind, offline: bool) -> Result<Vec<Status>> {
    let lock = read_lock(root)?;
    let mut libraries = Libraries::new(&lock);
    let mut statuses = Vec::new();
    for unit in Unit::all(&lock, kind)? {
        let (state, mut files) = unit.split(unit.local_states(root)?)?;
        files.retain(|_, state| *state != LocalState::Unchanged);
        let name = match state {
            LocalState::Missing => None,
            LocalState::Unchanged | LocalState::Edited => {
                Some(name_of(&read_text(root, &unit.document())?)?.to_owned())
            }
        };
        let newest = if offline {
            None
        } else {
            let upstream = libraries.get(&unit.source)?;
            let revision = upstream.newest(&unit.reference)?;
            let (change, files) =
                match upstream.folder(&revision.commit, kind, unit.origin.name())? {
                    None => (Change::Removed, BTreeMap::new()),
                    Some(next) => {
                        let (change, mut files) = unit.split(unit.changes(&next)?)?;
                        files.retain(|_, change| *change != Change::Current);
                        (change, files)
                    }
                };
            Some(Newest {
                reference: revision.reference,
                change,
                files,
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
pub fn update(
    root: &Path,
    kind: Kind,
    paths: &[PathBuf],
    reference: Option<&str>,
) -> Result<Vec<Updated>> {
    let mut lock = read_lock(root)?;
    ensure!(!lock.is_empty(), "nothing is recorded in {LOCK_FILE}");
    let units = select(&lock, kind, paths)?;
    let edits = units
        .iter()
        .map(|unit| unit.ensure_updatable(root))
        .collect::<Result<Vec<_>>>()?;
    let mut tracked = vec![PathBuf::from(LOCK_FILE)];
    tracked.extend(units.iter().flat_map(Unit::locked_paths));
    ensure!(
        !uncommitted(root, kind, &tracked)?,
        "commit or set aside changes to {LOCK_FILE} and the selected {} first",
        kind.name()
    );

    let mut libraries = Libraries::new(&lock);
    let mut writes = Vec::new();
    let mut recorded = Vec::new();
    let mut results = Vec::new();
    for (mut unit, edited) in units.into_iter().zip(edits) {
        let upstream = libraries.get(&unit.source)?;
        let revision = match reference {
            Some(reference) => upstream.resolve(Some(reference))?,
            None => upstream.newest(&unit.reference)?,
        };
        let name = unit.local_name(root)?;
        let Some(next) = upstream.folder(&revision.commit, kind, unit.origin.name())? else {
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
        files.retain(|_, outcome| *outcome != Outcome::Unchanged);
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

/// Return base, newest upstream, and local texts for one entry. Works while conflict markers remain.
pub fn diff(root: &Path, kind: Kind, path: &Path) -> Result<Comparison> {
    let lock = read_lock(root)?;
    let unit = Unit::find(&lock, kind, &kind.key(path)?)?;
    let document = unit.document();
    let local = read_text(root, &document)?;
    let name = name_of(&local)?;
    let mut libraries = Libraries::new(&lock);
    let upstream = libraries.get(&unit.source)?;
    let base = upstream
        .folder(&unit.commit, kind, unit.origin.name())?
        .with_context(|| unit.missing_at_lock())?;
    let revision = upstream.newest(&unit.reference)?;
    let next = upstream.folder(&revision.commit, kind, unit.origin.name())?;
    let renamed = |files: &Files| -> Result<String> {
        let file = files
            .get(Path::new(kind.document()))
            .with_context(|| format!("library copy has no {}", kind.document()))?;
        with_name(unit::text(&file.bytes, &document)?, name)
    };
    Ok(Comparison {
        base: Version {
            reference: unit.reference.clone(),
            commit: unit.commit.clone(),
            text: renamed(&base)?,
        },
        upstream: next
            .as_ref()
            .map(|next| {
                Ok::<_, anyhow::Error>(Version {
                    reference: revision.reference.clone(),
                    commit: revision.commit.clone(),
                    text: renamed(next)?,
                })
            })
            .transpose()?,
        local: local.clone(),
        files: unit.compare(root, &base, next.as_ref())?,
        path: unit.key,
    })
}

/// Drop a lock entry. Unless the files are kept as local files, delete the locked ones and the
/// folders they leave empty, refusing while Git has uncommitted changes to any of them.
pub fn remove(root: &Path, kind: Kind, path: &Path, keep_file: bool) -> Result<PathBuf> {
    let mut lock = read_lock(root)?;
    let key = kind.key(path)?;
    let unit = Unit::find(&lock, kind, &key)?;
    if !keep_file {
        ensure!(
            !uncommitted(root, kind, &unit.locked_paths().collect::<Vec<_>>())?,
            "commit or set aside changes to {} first, or keep its files with --keep-file",
            key.display()
        );
    }
    match kind {
        Kind::Contracts => {
            lock.contracts.remove(&key);
        }
        Kind::Skills => {
            lock.skills.remove(&key);
        }
    }
    if !keep_file {
        unit.delete(root)?;
    }
    write_lock(root, &lock)?;
    Ok(key)
}

/// Check the lock's format, that every locked file exists, that read-only files match their
/// hashes, and that installed skills' Markdown is valid. Works offline.
pub fn validate_lock(root: &Path) -> Result<Recorded> {
    let path = root.join(LOCK_FILE);
    if !path.exists() {
        return Ok(Recorded {
            contracts: 0,
            skills: 0,
        });
    }
    let lock = Lock::parse(&read_text(root, Path::new(LOCK_FILE))?)?;
    let units = Unit::all(&lock, Kind::Contracts)?
        .into_iter()
        .chain(Unit::all(&lock, Kind::Skills)?);
    for unit in units {
        let kind = unit.kind();
        for path in unit.check_files(root)? {
            let file = unit.folder.join(&path);
            if kind.rule(&path) == Rule::Merged
                && let Some(line) = conflict_marker(&read_text(root, &file)?)
            {
                bail!(
                    "{}:{line}: unresolved merge conflict marker",
                    file.display()
                );
            }
        }
        if kind == Kind::Skills {
            let document = unit.document();
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
    if !root.join(LOCK_FILE).exists() {
        return Ok(Lock::default());
    }
    Lock::parse(&read_text(root, Path::new(LOCK_FILE))?)
}

/// Write the lock, or delete it once it records nothing.
fn write_lock(root: &Path, lock: &Lock) -> Result<()> {
    let path = root.join(LOCK_FILE);
    let cannot = || format!("cannot write {LOCK_FILE}");
    if lock.is_empty() {
        if path.exists() {
            fs::remove_file(path).with_context(cannot)?;
        }
        return Ok(());
    }
    fs::write(path, lock.render()?).with_context(cannot)
}

/// Read a text file; `path` is relative to the project root.
fn read_text(root: &Path, path: &Path) -> Result<String> {
    fs::read_to_string(root.join(path)).with_context(|| format!("cannot read {}", path.display()))
}

/// Whether Git reports uncommitted changes to any of `paths`, which are relative to the project
/// root.
fn uncommitted(root: &Path, kind: Kind, paths: &[PathBuf]) -> Result<bool> {
    let paths = paths
        .iter()
        .map(|path| path.to_str().context("path is not UTF-8"))
        .collect::<Result<Vec<_>>>()?;
    let status = git::output(
        root,
        &[&["status", "--porcelain", "--"][..], &paths].concat(),
    )
    .with_context(|| format!("tenet {} needs a Git checkout", kind.name()))?;
    Ok(!status.is_empty())
}

/// Refuse to record paths that Git ignores, since Git would never commit them and every other
/// clone would fail `tenet validate`. `paths` are relative to the project root.
fn refuse_ignored(root: &Path, paths: &[PathBuf]) -> Result<()> {
    if let Some(ignored) = git::ignored(root, paths).context("Tenet needs a Git checkout")? {
        bail!(
            "Git ignores {} through {}; change that rule or install the copy elsewhere",
            ignored.path,
            ignored.rule
        );
    }
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
        tenet_contracts::is_kebab(source) && tenet_contracts::is_kebab(name),
        "expected SOURCE/NAME[@REF] with kebab-case names"
    );
    Ok((source, name, reference))
}

fn select(lock: &Lock, kind: Kind, paths: &[PathBuf]) -> Result<Vec<Unit>> {
    if paths.is_empty() {
        return Unit::all(lock, kind);
    }
    paths
        .iter()
        .map(|path| Unit::find(lock, kind, &kind.key(path)?))
        .collect()
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
