//! A lock entry of either kind, seen as an installed folder of locked files.

use alloc::collections::{BTreeMap, BTreeSet};
use core::{cmp::Reverse, str};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use tenet_contracts::{Entry, Lock, SkillEntry, hash_file, name_of, with_name};

use super::{
    AdoptionError,
    Change,
    FileComparison,
    LocalState,
    Origin,
    Outcome,
    kind::{Kind, Rule},
    upstream::{self, File, Files, Revision, Upstream},
};

/// Read a Markdown file's bytes as text.
pub(super) fn decode_text<'text>(
    bytes: &'text [u8],
    path: &Path,
) -> Result<&'text str, AdoptionError> {
    str::from_utf8(bytes).map_err(|source| {
        AdoptionError::NotUtf8Text {
            path: path.to_owned(),
            source,
        }
    })
}

/// Write one installed file, creating its folders; `path` is relative to the project root.
pub(super) fn write(root: &Path, path: &Path, file: &File) -> Result<(), AdoptionError> {
    let full = root.join(path);
    let cannot = |source| {
        AdoptionError::Write {
            path: path.to_owned(),
            source,
        }
    };
    fs::create_dir_all(full.parent().ok_or(AdoptionError::NoInstalledFolder)?).map_err(cannot)?;
    fs::write(&full, &file.bytes).map_err(cannot)?;
    #[cfg(unix)]
    if file.executable {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&full, fs::Permissions::from_mode(0o755)).map_err(cannot)?;
    }
    Ok(())
}

/// Read one installed file; `path` is relative to the project root.
pub(super) fn read(root: &Path, path: &Path) -> Result<Vec<u8>, AdoptionError> {
    fs::read(root.join(path)).map_err(|source| {
        AdoptionError::Read {
            path: path.to_owned(),
            source,
        }
    })
}

/// The first existing path that keeps a new file from being written at `folder/path`: anything
/// at that path, or a file or symlink where a folder between `folder` and that path belongs.
///
/// `folder` is relative to the project root, and `path` to `folder`.
pub(super) fn find_obstruction(
    root: &Path,
    folder: &Path,
    path: &Path,
) -> Result<Option<PathBuf>, AdoptionError> {
    let target = folder.join(path);
    let mut current = folder.to_owned();
    for part in path.components() {
        current.push(part);
        let metadata = match root.join(&current).symlink_metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(AdoptionError::Inspect {
                    path: current,
                    source,
                });
            },
        };
        if current == target || !metadata.is_dir() {
            return Ok(Some(current));
        }
    }
    Ok(None)
}

/// A lock entry, contract or skill, seen as an installed folder of locked files.
pub(super) struct Unit {
    pub commit: String,
    /// Locked hashes keyed by path relative to the folder, the CONTRACT.md or SKILL.md included.
    pub files: BTreeMap<PathBuf, String>,
    /// Relative to the project root.
    pub folder: PathBuf,
    /// The lock key: a contract's CONTRACT.md path or a skill's folder.
    pub key: PathBuf,
    pub origin: Origin,
    pub reference: String,
    pub source: String,
}

/// An update decided for one copy before anything is written.
pub(super) struct Planned {
    /// The locked hashes at the new release.
    pub files: BTreeMap<PathBuf, String>,
    /// Each file's outcome, keyed by path relative to the folder.
    pub outcomes: BTreeMap<PathBuf, Outcome>,
    /// Contents to write, keyed by path relative to the project root.
    pub writes: Vec<(PathBuf, File)>,
}

impl Unit {
    /// Refuse a missing locked file or an edited read-only one, as `validate` and `update` do, and
    /// return the edited Markdown files, which merge.
    pub(super) fn check_files(&self, root: &Path) -> Result<BTreeSet<PathBuf>, AdoptionError> {
        let kind = self.read_kind();
        let mut edited_paths = BTreeSet::new();
        for (path, state) in self.read_local_states(root)? {
            let file = self.folder.join(&path);
            match (state, kind.derive_rule(&path)) {
                (LocalState::Missing, _) => {
                    return Err(AdoptionError::MissingFile {
                        file,
                        kind,
                        key: self.key.clone(),
                    });
                },
                (LocalState::Edited, Rule::ReadOnly) => {
                    return Err(AdoptionError::EditedReadOnly {
                        file,
                        kind,
                        key: self.key.clone(),
                    });
                },
                (LocalState::Edited, Rule::Named | Rule::Merged) => {
                    let _: bool = edited_paths.insert(path);
                },
                (LocalState::Unchanged, _) => {},
            }
        }
        Ok(edited_paths)
    }

    /// The files other than the CONTRACT.md or SKILL.md that differ between the locked release,
    /// the newest release, and the local copy.
    pub(super) fn compare(
        &self,
        root: &Path,
        base: &Files,
        next: Option<&Files>,
    ) -> Result<Vec<FileComparison>, AdoptionError> {
        let kind = self.read_kind();
        let paths: BTreeSet<&PathBuf> = base
            .keys()
            .chain(next.into_iter().flat_map(Files::keys))
            .filter(|path| kind.derive_rule(path) != Rule::Named)
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
                base.get(path).map(|base_file| base_file.bytes.as_slice()),
                next.and_then(|next_files| next_files.get(path))
                    .map(|next_file| next_file.bytes.as_slice()),
                local.as_deref(),
            ];
            let hashes = versions.map(|version| version.map(hash_file));
            if hashes.iter().all(|hash| *hash == hashes[0]) {
                continue;
            }
            let [base_bytes, upstream, local_bytes] = versions
                .map(|version| version.map(|bytes| String::from_utf8_lossy(bytes).into_owned()));
            comparisons.push(FileComparison {
                path: path.clone(),
                base: base_bytes,
                upstream,
                local: local_bytes,
            });
        }
        Ok(comparisons)
    }

    /// Delete the locked files, then each folder they leave empty, up to and including the copy's
    /// folder. Files the lock does not list stay.
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    pub(super) fn delete(&self, root: &Path) -> Result<(), AdoptionError> {
        let folder = root.join(&self.folder);
        let mut directories = BTreeSet::new();
        for path in self.files.keys() {
            let full = folder.join(path);
            if full.exists() {
                fs::remove_file(&full).map_err(|source| {
                    AdoptionError::Remove {
                        path: self.folder.join(path),
                        source,
                    }
                })?;
            }
            directories.extend(
                full.ancestors()
                    .skip(1)
                    .take_while(|directory| directory.starts_with(&folder))
                    .map(Path::to_owned),
            );
        }
        let mut sorted: Vec<_> = directories.into_iter().collect();
        sorted.sort_by_key(|directory| Reverse(directory.components().count()));
        for directory in sorted {
            let cannot = |source| {
                AdoptionError::Remove {
                    path: directory.clone(),
                    source,
                }
            };
            if directory.is_dir() && directory.read_dir().map_err(cannot)?.next().is_none() {
                fs::remove_dir(&directory).map_err(cannot)?;
            }
        }
        Ok(())
    }

    /// The local name, after validating the CONTRACT.md or SKILL.md as an update reads it.
    pub(super) fn derive_local_name(&self, root: &Path) -> Result<String, AdoptionError> {
        let document = self.name_document();
        let text = fs::read_to_string(root.join(&document)).map_err(|source| {
            AdoptionError::Read {
                path: document.clone(),
                source,
            }
        })?;
        self.read_kind().validate(&document, &text)?;
        Ok(name_of(&text)?.to_owned())
    }

    /// Refuse an update, before anything is fetched or written, as [`Unit::check_files`] does,
    /// and return the edited Markdown files.
    pub(super) fn ensure_updatable(&self, root: &Path) -> Result<BTreeSet<PathBuf>, AdoptionError> {
        let document = self.name_document();
        if !root.join(&document).is_file() {
            return Err(AdoptionError::DocumentMissing {
                document,
                kind: self.read_kind(),
                key: self.key.clone(),
            });
        }
        self.check_files(root)
    }

    pub(super) fn find(lock: &Lock, kind: Kind, key: &Path) -> Result<Self, AdoptionError> {
        let unit = match kind {
            Kind::Contracts => {
                lock.contracts
                    .get(key)
                    .map(|entry| Self::from_contract(key, entry))
                    .transpose()?
            },
            Kind::Skills => {
                lock.skills
                    .get(key)
                    .map(|entry| Self::from_skill(key, entry))
            },
        };
        unit.ok_or_else(|| AdoptionError::NotRecorded(key.to_owned()))
    }

    fn from_contract(path: &Path, entry: &Entry) -> Result<Self, AdoptionError> {
        let mut files = entry.files.clone();
        drop(files.insert(Kind::Contracts.name_document().into(), entry.hash.clone()));
        Ok(Self {
            commit: entry.commit.clone(),
            files,
            folder: path
                .parent()
                .ok_or(AdoptionError::NoContractFolder)?
                .to_owned(),
            key: path.to_owned(),
            origin: Origin::Contract(entry.contract.clone()),
            reference: entry.reference.clone(),
            source: entry.source.clone(),
        })
    }

    fn from_skill(folder: &Path, entry: &SkillEntry) -> Self {
        Self {
            commit: entry.commit.clone(),
            files: entry.files.clone(),
            folder: folder.to_owned(),
            key: folder.to_owned(),
            origin: Origin::Skill(entry.skill.clone()),
            reference: entry.reference.clone(),
            source: entry.source.clone(),
        }
    }

    /// Every entry of `kind`, in key order.
    pub(super) fn list_all(lock: &Lock, kind: Kind) -> Result<Vec<Self>, AdoptionError> {
        match kind {
            Kind::Contracts => {
                lock.contracts
                    .iter()
                    .map(|(path, entry)| Self::from_contract(path, entry))
                    .collect()
            },
            Kind::Skills => {
                Ok(lock
                    .skills
                    .iter()
                    .map(|(folder, entry)| Self::from_skill(folder, entry))
                    .collect())
            },
        }
    }

    /// Every locked file, relative to the project root.
    pub(super) fn list_locked_paths(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.files.keys().map(|path| self.folder.join(path))
    }

    /// The CONTRACT.md or SKILL.md, relative to the project root.
    pub(super) fn name_document(&self) -> PathBuf {
        self.folder.join(self.read_kind().name_document())
    }

    /// Compare upstream's files at a newer release with the locked ones.
    pub(super) fn plan_changes(
        &self,
        next: &Files,
    ) -> Result<BTreeMap<PathBuf, Change>, AdoptionError> {
        let kind = self.read_kind();
        let mut changes = BTreeMap::new();
        for (path, file) in next {
            let change = match self.files.get(path) {
                None => Change::Added,
                Some(locked) if *locked == kind.hash(path, &file.bytes)? => Change::Current,
                Some(_) => Change::Changed,
            };
            let _: Option<Change> = changes.insert(path.clone(), change);
        }
        for path in self.files.keys().filter(|path| !next.contains_key(*path)) {
            let _: Option<Change> = changes.insert(path.clone(), Change::Removed);
        }
        Ok(changes)
    }

    /// Decide how each file takes the release `next`, without writing anything. `edited_paths`
    /// holds the edited Markdown files, as [`Unit::ensure_updatable`] found them; every other
    /// locked file matches its hash. An unedited file is replaced, edited Markdown merges three
    /// ways against the locked commit, a new upstream file is added, and a file upstream removed
    /// stays in place and leaves the lock. Refuses a local file or symlink in the way of a new
    /// upstream file.
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "The steps run in one order and share local state; splitting them would add \
                  forwarding functions."
    )]
    pub(super) fn plan_update(
        &self,
        root: &Path,
        name: &str,
        edited_paths: &BTreeSet<PathBuf>,
        upstream: &Upstream,
        revision: &Revision,
        next: Files,
    ) -> Result<Planned, AdoptionError> {
        let kind = self.read_kind();
        let mut base: Option<Files> = None;
        let mut planned = Planned {
            files: BTreeMap::new(),
            outcomes: BTreeMap::new(),
            writes: Vec::new(),
        };
        for (path, mut file) in next {
            let rule = kind.derive_rule(&path);
            let installed = self.folder.join(&path);
            if rule == Rule::Named {
                let document = with_name(decode_text(&file.bytes, &installed)?, name)?;
                // Report the upstream text's error as a message, so it never points into the local
                // file.
                kind.validate(&installed, &document).map_err(|error| {
                    AdoptionError::UnreadableRelease {
                        reference: revision.reference.clone(),
                        reason: match kind {
                            Kind::Contracts => "needs a newer Tenet",
                            Kind::Skills => "has an invalid SKILL.md",
                        },
                        error: Box::new(error),
                    }
                })?;
                file.bytes = document.into_bytes();
            }
            let next_hash = kind.hash(&path, &file.bytes)?;

            let outcome = match self.files.get(&path) {
                Some(locked) if *locked == next_hash => Outcome::Unchanged,
                Some(_) if !edited_paths.contains(&path) => {
                    planned.writes.push((installed, file));
                    Outcome::Replaced
                },
                Some(_) => {
                    let local = read(root, &installed)?;
                    if base.is_none() {
                        base = Some(
                            upstream
                                .read_folder(&self.commit, kind, self.origin.name())?
                                .ok_or_else(|| self.report_missing_at_lock())?,
                        );
                    }
                    let base_file = base
                        .as_ref()
                        .and_then(|base_files| base_files.get(&path))
                        .ok_or_else(|| {
                            AdoptionError::MissingAtLock {
                                commit: self.commit.clone(),
                                name: path.display().to_string(),
                            }
                        })?;
                    let mut base_text = decode_text(&base_file.bytes, &installed)?.to_owned();
                    if rule == Rule::Named {
                        base_text = with_name(&base_text, name)?;
                    }
                    let merge = upstream::merge(
                        decode_text(&local, &installed)?,
                        &base_text,
                        decode_text(&file.bytes, &installed)?,
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
                },
                None => {
                    match find_obstruction(root, &self.folder, &path)? {
                        None => {},
                        Some(found) if found == installed => {
                            return Err(AdoptionError::UnrecordedAtTarget {
                                path: installed,
                                reference: revision.reference.clone(),
                            });
                        },
                        Some(found) => {
                            return Err(AdoptionError::InTheWayOfUpdate {
                                found,
                                path: installed,
                                reference: revision.reference.clone(),
                            });
                        },
                    }
                    planned.writes.push((installed, file));
                    Outcome::Added
                },
            };
            let _: Option<Outcome> = planned.outcomes.insert(path.clone(), outcome);
            drop(planned.files.insert(path, next_hash));
        }
        for path in self.files.keys() {
            if !planned.files.contains_key(path) {
                let _: Option<Outcome> = planned.outcomes.insert(path.clone(), Outcome::Removed);
            }
        }
        Ok(planned)
    }

    pub(super) const fn read_kind(&self) -> Kind {
        self.origin.read_kind()
    }

    /// Each locked file's local state.
    pub(super) fn read_local_states(
        &self,
        root: &Path,
    ) -> Result<BTreeMap<PathBuf, LocalState>, AdoptionError> {
        let kind = self.read_kind();
        self.files
            .iter()
            .map(|(path, locked)| {
                let file = self.folder.join(path);
                let state = if !root.join(&file).exists() {
                    LocalState::Missing
                } else if kind.hash(path, &read(root, &file)?).map_err(|source| {
                    AdoptionError::HashFile {
                        path: file.clone(),
                        source,
                    }
                })? == *locked
                {
                    LocalState::Unchanged
                } else {
                    LocalState::Edited
                };
                Ok((path.clone(), state))
            })
            .collect()
    }

    /// The error for a library that has no copy of this entry at its locked commit.
    pub(super) fn report_missing_at_lock(&self) -> AdoptionError {
        AdoptionError::MissingAtLock {
            commit: self.commit.clone(),
            name: self.origin.name().to_owned(),
        }
    }

    /// Separate the CONTRACT.md or SKILL.md's value from the other files' values.
    pub(super) fn split<Value>(
        &self,
        mut values: BTreeMap<PathBuf, Value>,
    ) -> Result<(Value, BTreeMap<PathBuf, Value>), AdoptionError> {
        let document = values
            .remove(Path::new(self.read_kind().name_document()))
            .ok_or_else(|| {
                AdoptionError::NoDocument {
                    key: self.key.clone(),
                    document: self.read_kind().name_document(),
                }
            })?;
        Ok((document, values))
    }

    /// Record this copy in the lock, replacing any entry under its key.
    pub(super) fn store(self, lock: &mut Lock) -> Result<(), AdoptionError> {
        match self.origin {
            Origin::Contract(contract) => {
                let mut files = self.files;
                let hash = files
                    .remove(Path::new(Kind::Contracts.name_document()))
                    .ok_or(AdoptionError::NoContractDocument)?;
                drop(lock.contracts.insert(
                    self.key,
                    Entry {
                        source: self.source,
                        contract,
                        reference: self.reference,
                        commit: self.commit,
                        hash,
                        files,
                    },
                ));
            },
            Origin::Skill(skill) => {
                drop(lock.skills.insert(
                    self.key,
                    SkillEntry {
                        source: self.source,
                        skill,
                        reference: self.reference,
                        commit: self.commit,
                        files: self.files,
                    },
                ));
            },
        }
        Ok(())
    }
}
