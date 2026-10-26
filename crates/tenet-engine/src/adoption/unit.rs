//! A lock entry of either kind, seen as an installed folder of locked files.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use tenet_contracts::{Entry, LOCK_FILE, Lock, SkillEntry, file_hash, name_of, with_name};

use super::{
    Change, FileComparison, LocalState, Origin, Outcome,
    kind::{Kind, Rule},
    upstream::{self, File, Files, Revision, Upstream},
};

/// Read a Markdown file's bytes as text.
pub(super) fn text<'a>(bytes: &'a [u8], path: &Path) -> Result<&'a str> {
    std::str::from_utf8(bytes).with_context(|| format!("{} is not UTF-8", path.display()))
}

/// Write one installed file, creating its folders; `path` is relative to the project root.
pub(super) fn write(root: &Path, path: &Path, file: &File) -> Result<()> {
    let full = root.join(path);
    let cannot = || format!("cannot write {}", path.display());
    fs::create_dir_all(full.parent().context("installed file has no folder")?)
        .with_context(cannot)?;
    fs::write(&full, &file.bytes).with_context(cannot)?;
    #[cfg(unix)]
    if file.executable {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&full, fs::Permissions::from_mode(0o755)).with_context(cannot)?;
    }
    Ok(())
}

/// Read one installed file; `path` is relative to the project root.
pub(super) fn read(root: &Path, path: &Path) -> Result<Vec<u8>> {
    fs::read(root.join(path)).with_context(|| format!("cannot read {}", path.display()))
}

/// The first existing path that keeps a new file from being written at `folder/path`: anything
/// at that path, or a file or symlink where a folder between `folder` and that path belongs.
/// `folder` is relative to the project root, and `path` to `folder`.
pub(super) fn obstruction(root: &Path, folder: &Path, path: &Path) -> Result<Option<PathBuf>> {
    let target = folder.join(path);
    let mut current = folder.to_owned();
    for part in path.components() {
        current.push(part);
        let metadata = match root.join(&current).symlink_metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("cannot inspect {}", current.display()));
            }
        };
        if current == target || !metadata.is_dir() {
            return Ok(Some(current));
        }
    }
    Ok(None)
}

/// A lock entry, contract or skill, seen as an installed folder of locked files.
pub(super) struct Unit {
    pub origin: Origin,
    /// The lock key: a contract's CONTRACT.md path or a skill's folder.
    pub key: PathBuf,
    /// Relative to the project root.
    pub folder: PathBuf,
    pub source: String,
    pub reference: String,
    pub commit: String,
    /// Locked hashes keyed by path relative to the folder, the CONTRACT.md or SKILL.md included.
    pub files: BTreeMap<PathBuf, String>,
}

/// An update decided for one copy before anything is written.
pub(super) struct Planned {
    /// Each file's outcome, keyed by path relative to the folder.
    pub outcomes: BTreeMap<PathBuf, Outcome>,
    /// Contents to write, keyed by path relative to the project root.
    pub writes: Vec<(PathBuf, File)>,
    /// The locked hashes at the new release.
    pub files: BTreeMap<PathBuf, String>,
}

impl Unit {
    /// Every entry of `kind`, in key order.
    pub(super) fn all(lock: &Lock, kind: Kind) -> Result<Vec<Self>> {
        match kind {
            Kind::Contracts => lock
                .contracts
                .iter()
                .map(|(path, entry)| Self::contract(path, entry))
                .collect(),
            Kind::Skills => Ok(lock
                .skills
                .iter()
                .map(|(folder, entry)| Self::skill(folder, entry))
                .collect()),
        }
    }

    pub(super) fn find(lock: &Lock, kind: Kind, key: &Path) -> Result<Self> {
        let unit = match kind {
            Kind::Contracts => lock
                .contracts
                .get(key)
                .map(|entry| Self::contract(key, entry))
                .transpose()?,
            Kind::Skills => lock.skills.get(key).map(|entry| Self::skill(key, entry)),
        };
        unit.with_context(|| format!("{} is not recorded in {LOCK_FILE}", key.display()))
    }

    fn contract(path: &Path, entry: &Entry) -> Result<Self> {
        let mut files = entry.files.clone();
        files.insert(Kind::Contracts.document().into(), entry.hash.clone());
        Ok(Self {
            origin: Origin::Contract(entry.contract.clone()),
            key: path.to_owned(),
            folder: path.parent().context("contract has no folder")?.to_owned(),
            source: entry.source.clone(),
            reference: entry.reference.clone(),
            commit: entry.commit.clone(),
            files,
        })
    }

    fn skill(folder: &Path, entry: &SkillEntry) -> Self {
        Self {
            origin: Origin::Skill(entry.skill.clone()),
            key: folder.to_owned(),
            folder: folder.to_owned(),
            source: entry.source.clone(),
            reference: entry.reference.clone(),
            commit: entry.commit.clone(),
            files: entry.files.clone(),
        }
    }

    /// Record this copy in the lock, replacing any entry under its key.
    pub(super) fn store(self, lock: &mut Lock) -> Result<()> {
        match self.origin {
            Origin::Contract(contract) => {
                let mut files = self.files;
                let hash = files
                    .remove(Path::new(Kind::Contracts.document()))
                    .context("a contract's files must include its CONTRACT.md")?;
                lock.contracts.insert(
                    self.key,
                    Entry {
                        source: self.source,
                        contract,
                        reference: self.reference,
                        commit: self.commit,
                        hash,
                        files,
                    },
                );
            }
            Origin::Skill(skill) => {
                lock.skills.insert(
                    self.key,
                    SkillEntry {
                        source: self.source,
                        skill,
                        reference: self.reference,
                        commit: self.commit,
                        files: self.files,
                    },
                );
            }
        }
        Ok(())
    }

    pub(super) fn kind(&self) -> Kind {
        self.origin.kind()
    }

    /// The CONTRACT.md or SKILL.md, relative to the project root.
    pub(super) fn document(&self) -> PathBuf {
        self.folder.join(self.kind().document())
    }

    /// Every locked file, relative to the project root.
    pub(super) fn locked_paths(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.files.keys().map(|path| self.folder.join(path))
    }

    /// Separate the CONTRACT.md or SKILL.md's value from the other files' values.
    pub(super) fn split<T>(
        &self,
        mut values: BTreeMap<PathBuf, T>,
    ) -> Result<(T, BTreeMap<PathBuf, T>)> {
        let document = values
            .remove(Path::new(self.kind().document()))
            .with_context(|| format!("{} has no {}", self.key.display(), self.kind().document()))?;
        Ok((document, values))
    }

    /// Each locked file's local state.
    pub(super) fn local_states(&self, root: &Path) -> Result<BTreeMap<PathBuf, LocalState>> {
        let kind = self.kind();
        self.files
            .iter()
            .map(|(path, locked)| {
                let file = self.folder.join(path);
                let state = if !root.join(&file).exists() {
                    LocalState::Missing
                } else if kind
                    .hash(path, &read(root, &file)?)
                    .with_context(|| format!("cannot hash {}", file.display()))?
                    == *locked
                {
                    LocalState::Unchanged
                } else {
                    LocalState::Edited
                };
                Ok((path.clone(), state))
            })
            .collect()
    }

    /// Refuse a missing locked file or an edited read-only one, as `validate` and `update` do, and
    /// return the edited Markdown files, which merge.
    pub(super) fn check_files(&self, root: &Path) -> Result<BTreeSet<PathBuf>> {
        let kind = self.kind();
        let mut edited = BTreeSet::new();
        for (path, state) in self.local_states(root)? {
            let file = self.folder.join(&path);
            match (state, kind.rule(&path)) {
                (LocalState::Missing, _) => bail!(
                    "{} is recorded in {LOCK_FILE} but missing; restore it from Git, and compare it \
                     with upstream using `tenet {} diff {}`",
                    file.display(),
                    kind.name(),
                    self.key.display()
                ),
                (LocalState::Edited, Rule::ReadOnly) => bail!(
                    "{} does not match its hash in {LOCK_FILE}, and Tenet never overwrites a read-only \
                     file; compare it with upstream using `tenet {} diff {}`, restore it, and keep \
                     your changes in a file of your own",
                    file.display(),
                    kind.name(),
                    self.key.display()
                ),
                (LocalState::Edited, Rule::Named | Rule::Merged) => {
                    edited.insert(path);
                }
                (LocalState::Unchanged, _) => {}
            }
        }
        Ok(edited)
    }

    /// Refuse an update, before anything is fetched or written, as [`Unit::check_files`] does,
    /// and return the edited Markdown files.
    pub(super) fn ensure_updatable(&self, root: &Path) -> Result<BTreeSet<PathBuf>> {
        let document = self.document();
        ensure!(
            root.join(&document).is_file(),
            "{} is missing; restore it, fix its path in {LOCK_FILE}, or run `tenet {} remove {}`",
            document.display(),
            self.kind().name(),
            self.key.display()
        );
        self.check_files(root)
    }

    /// The local name, after validating the CONTRACT.md or SKILL.md as an update reads it.
    pub(super) fn local_name(&self, root: &Path) -> Result<String> {
        let document = self.document();
        let text = fs::read_to_string(root.join(&document))
            .with_context(|| format!("cannot read {}", document.display()))?;
        self.kind().validate(&document, &text)?;
        Ok(name_of(&text)?.to_owned())
    }

    /// Compare upstream's files at a newer release with the locked ones.
    pub(super) fn changes(&self, next: &Files) -> Result<BTreeMap<PathBuf, Change>> {
        let kind = self.kind();
        let mut changes = BTreeMap::new();
        for (path, file) in next {
            let change = match self.files.get(path) {
                None => Change::Added,
                Some(locked) if *locked == kind.hash(path, &file.bytes)? => Change::Current,
                Some(_) => Change::Changed,
            };
            changes.insert(path.clone(), change);
        }
        for path in self.files.keys().filter(|path| !next.contains_key(*path)) {
            changes.insert(path.clone(), Change::Removed);
        }
        Ok(changes)
    }

    /// Decide how each file takes the release `next`, without writing anything. `edited` holds the
    /// edited Markdown files, as [`Unit::ensure_updatable`] found them; every other locked file
    /// matches its hash. An unedited file is replaced, edited Markdown merges three ways against
    /// the locked commit, a new upstream file is added, and a file upstream removed stays in place
    /// and leaves the lock. Refuses a local file or symlink in the way of a new upstream file.
    pub(super) fn plan_update(
        &self,
        root: &Path,
        name: &str,
        edited: &BTreeSet<PathBuf>,
        upstream: &Upstream,
        revision: &Revision,
        next: Files,
    ) -> Result<Planned> {
        let kind = self.kind();
        let mut base: Option<Files> = None;
        let mut planned = Planned {
            outcomes: BTreeMap::new(),
            writes: Vec::new(),
            files: BTreeMap::new(),
        };
        for (path, mut file) in next {
            let rule = kind.rule(&path);
            let installed = self.folder.join(&path);
            if rule == Rule::Named {
                let document = with_name(text(&file.bytes, &installed)?, name)?;
                // Report the upstream text's error as a message, so it never points into the local file.
                kind.validate(&installed, &document).map_err(|error| {
                    let reason = match kind {
                        Kind::Contracts => "needs a newer Tenet",
                        Kind::Skills => "has an invalid SKILL.md",
                    };
                    anyhow!("{} {reason}: {error:#}", revision.reference)
                })?;
                file.bytes = document.into_bytes();
            }
            let next_hash = kind.hash(&path, &file.bytes)?;

            let outcome = match self.files.get(&path) {
                Some(locked) if *locked == next_hash => Outcome::Unchanged,
                Some(_) if !edited.contains(&path) => {
                    planned.writes.push((installed, file));
                    Outcome::Replaced
                }
                Some(_) => {
                    let local = read(root, &installed)?;
                    if base.is_none() {
                        base = Some(
                            upstream
                                .folder(&self.commit, kind, self.origin.name())?
                                .with_context(|| self.missing_at_lock())?,
                        );
                    }
                    let base_file = base
                        .as_ref()
                        .and_then(|base| base.get(&path))
                        .with_context(|| {
                            format!(
                                "the locked commit {} has no {}",
                                self.commit,
                                path.display()
                            )
                        })?;
                    let mut base_text = text(&base_file.bytes, &installed)?.to_owned();
                    if rule == Rule::Named {
                        base_text = with_name(&base_text, name)?;
                    }
                    let merge = upstream::merge(
                        text(&local, &installed)?,
                        &base_text,
                        text(&file.bytes, &installed)?,
                        [
                            "local",
                            &format!("base {}", self.reference),
                            &format!("upstream {}", revision.reference),
                        ],
                    )?;
                    planned.writes.push((
                        installed,
                        File {
                            bytes: merge.text.into_bytes(),
                            executable: file.executable,
                        },
                    ));
                    if merge.conflicts == 0 {
                        Outcome::Merged
                    } else {
                        Outcome::Conflict {
                            regions: merge.conflicts,
                        }
                    }
                }
                None => {
                    match obstruction(root, &self.folder, &path)? {
                        None => {}
                        Some(found) if found == installed => bail!(
                            "{} is not recorded in {LOCK_FILE}, but {} adds a file there; move it aside \
                             and update again",
                            installed.display(),
                            revision.reference
                        ),
                        Some(found) => bail!(
                            "{} is in the way of {}, which {} adds; move it aside and update again",
                            found.display(),
                            installed.display(),
                            revision.reference
                        ),
                    }
                    planned.writes.push((installed, file));
                    Outcome::Added
                }
            };
            planned.outcomes.insert(path.clone(), outcome);
            planned.files.insert(path, next_hash);
        }
        for path in self.files.keys() {
            if !planned.files.contains_key(path) {
                planned.outcomes.insert(path.clone(), Outcome::Removed);
            }
        }
        Ok(planned)
    }

    /// The error for a library that has no copy of this entry at its locked commit.
    pub(super) fn missing_at_lock(&self) -> String {
        format!(
            "the locked commit {} has no {}",
            self.commit,
            self.origin.name()
        )
    }

    /// The files other than the CONTRACT.md or SKILL.md that differ between the locked release,
    /// the newest release, and the local copy.
    pub(super) fn compare(
        &self,
        root: &Path,
        base: &Files,
        next: Option<&Files>,
    ) -> Result<Vec<FileComparison>> {
        let kind = self.kind();
        let paths: BTreeSet<&PathBuf> = base
            .keys()
            .chain(next.into_iter().flat_map(Files::keys))
            .filter(|path| kind.rule(path) != Rule::Named)
            .collect();
        let mut comparisons = Vec::new();
        for path in paths {
            let file = self.folder.join(path);
            let local = root
                .join(&file)
                .exists()
                .then(|| read(root, &file))
                .transpose()?;
            let versions = [
                base.get(path).map(|file| file.bytes.as_slice()),
                next.and_then(|next| next.get(path))
                    .map(|file| file.bytes.as_slice()),
                local.as_deref(),
            ];
            let hashes = versions.map(|version| version.map(file_hash));
            if hashes.iter().all(|hash| *hash == hashes[0]) {
                continue;
            }
            let [base, upstream, local] = versions
                .map(|version| version.map(|bytes| String::from_utf8_lossy(bytes).into_owned()));
            comparisons.push(FileComparison {
                path: path.clone(),
                base,
                upstream,
                local,
            });
        }
        Ok(comparisons)
    }

    /// Delete the locked files, then each folder they leave empty, up to and including the copy's
    /// folder. Files the lock does not list stay.
    pub(super) fn delete(&self, root: &Path) -> Result<()> {
        let folder = root.join(&self.folder);
        let mut directories = BTreeSet::new();
        for path in self.files.keys() {
            let full = folder.join(path);
            if full.exists() {
                fs::remove_file(&full).with_context(|| {
                    format!("cannot remove {}", self.folder.join(path).display())
                })?;
            }
            directories.extend(
                full.ancestors()
                    .skip(1)
                    .take_while(|directory| directory.starts_with(&folder))
                    .map(Path::to_owned),
            );
        }
        let mut directories: Vec<_> = directories.into_iter().collect();
        directories.sort_by_key(|directory| std::cmp::Reverse(directory.components().count()));
        for directory in directories {
            let cannot = || format!("cannot remove {}", directory.display());
            if directory.is_dir() && directory.read_dir().with_context(cannot)?.next().is_none() {
                fs::remove_dir(&directory).with_context(cannot)?;
            }
        }
        Ok(())
    }
}
