use alloc::collections::BTreeMap;
use core::str;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use tempfile::TempDir;
use tenet_contracts::is_contract_asset;

use super::{AdoptionError, Kind};
use crate::git::{self, GitError};

/// A temporary bare clone of a contract library. Nothing persists after the command.
pub(super) struct Upstream {
    _directory: TempDir,
    repository: PathBuf,
    url: String,
}

/// A requested reference and the commit it resolved to.
pub(super) struct Revision {
    pub commit: String,
    pub reference: String,
}

/// One file of a library folder, as Git stores it.
#[derive(Clone)]
pub(super) struct File {
    pub bytes: Vec<u8>,
    pub executable: bool,
}

/// A library folder's files, keyed by path relative to the folder.
pub(super) type Files = BTreeMap<PathBuf, File>;

impl Upstream {
    pub(super) fn download(url: &str) -> Result<Self, AdoptionError> {
        if url.starts_with('-') {
            return Err(AdoptionError::OptionUrl);
        }
        let directory = tempfile::tempdir().map_err(AdoptionError::Scratch)?;
        drop(
            git::run_git(
                directory.path(),
                &["clone", "--bare", "--quiet", "--", url, "library.git"],
            )
            .map_err(|source| {
                AdoptionError::ReadLibrary {
                    url: url.to_owned(),
                    source,
                }
            })?,
        );
        let repository = directory.path().join("library.git");
        Ok(Self {
            _directory: directory,
            repository,
            url: url.into(),
        })
    }

    fn find_latest_tag(&self) -> Result<String, AdoptionError> {
        self.list_releases()?
            .pop_last()
            .map(|(_, tag)| tag)
            .ok_or_else(|| AdoptionError::NoReleases(self.url.clone()))
    }

    /// Move a version tag to the highest version tag; a branch or commit resolves to itself.
    pub(super) fn find_newest(&self, locked: &str) -> Result<Revision, AdoptionError> {
        if parse_version(locked).is_some() {
            self.resolve(None)
        } else {
            self.resolve(Some(locked))
        }
    }

    /// The highest version tag of each major after a locked version tag, oldest first: the
    /// releases `update` stops at. Empty for a branch or commit.
    pub(super) fn list_majors(&self, locked: &str) -> Result<Vec<String>, AdoptionError> {
        let Some((locked_major, ..)) = parse_version(locked) else {
            return Ok(Vec::new());
        };
        // Releases come in version order, so each major keeps its highest_tags.
        let highest_tags = self
            .list_releases()?
            .into_iter()
            .filter(|((major, ..), _)| *major > locked_major)
            .map(|((major, ..), tag)| (major, tag))
            .collect::<BTreeMap<_, _>>();
        Ok(highest_tags.into_values().collect())
    }

    /// Every `vX.Y.Z` tag by its version.
    fn list_releases(&self) -> Result<BTreeMap<Number, String>, AdoptionError> {
        let tags = String::from_utf8(git::run_git(
            &self.repository,
            &["for-each-ref", "--format=%(refname:short)", "refs/tags"],
        )?)?;
        Ok(tags
            .lines()
            .filter_map(|tag| parse_version(tag).map(|number| (number, tag.to_owned())))
            .collect())
    }

    /// Read every file of `contracts/NAME/` or `skills/NAME/` at a commit. `None` when the folder
    /// has no CONTRACT.md or SKILL.md. Refuses a contract file other than CONTRACT.md and
    /// `assets/<tool>/` files, which this Tenet cannot install whole.
    pub(super) fn read_folder(
        &self,
        commit: &str,
        kind: Kind,
        name: &str,
    ) -> Result<Option<Files>, AdoptionError> {
        let prefix = format!("{}/{name}/", kind.name());
        let listing = git::run_git(
            &self.repository,
            &["ls-tree", "-r", "-z", commit, "--", &prefix],
        )?;
        let mut files = Files::new();
        for entry in listing
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
        {
            let line = str::from_utf8(entry).map_err(AdoptionError::LibraryPathNotUtf8)?;
            let (meta, path) = line
                .split_once('\t')
                .ok_or(AdoptionError::MalformedListing)?;
            let Some(relative) = path.strip_prefix(&prefix).map(PathBuf::from) else {
                continue;
            };
            if !(kind == Kind::Skills
                || relative == Path::new(kind.name_document())
                || is_contract_asset(&relative))
            {
                return Err(AdoptionError::UnsupportedFile {
                    path: path.to_owned(),
                    url: self.url.clone(),
                });
            }
            let mut fields = meta.split(' ');
            let executable = match fields.next() {
                Some("100644") => false,
                Some("100755") => true,
                _ => {
                    return Err(AdoptionError::IrregularFile {
                        path: path.to_owned(),
                        url: self.url.clone(),
                    });
                },
            };
            let object = fields.nth(1).ok_or(AdoptionError::MalformedListing)?;
            let bytes = git::run_git(&self.repository, &["cat-file", "blob", object])?;
            drop(files.insert(relative, File { bytes, executable }));
        }
        Ok(files
            .contains_key(Path::new(kind.name_document()))
            .then_some(files))
    }

    /// Resolve a tag, branch, or commit; without one, take the highest `vX.Y.Z` tag.
    pub(super) fn resolve(&self, reference: Option<&str>) -> Result<Revision, AdoptionError> {
        let resolved = match reference {
            Some(given) => given.to_owned(),
            None => self.find_latest_tag()?,
        };
        if resolved.starts_with('-') {
            return Err(AdoptionError::OptionReference);
        }
        let commit = git::run_git(
            &self.repository,
            &["rev-parse", "--verify", &format!("{resolved}^{{commit}}")],
        )
        .map_err(|source| {
            AdoptionError::UnknownReference {
                reference: resolved.clone(),
                url: self.url.clone(),
                source,
            }
        })?;
        Ok(Revision {
            commit: String::from_utf8(commit)?.trim().to_owned(),
            reference: resolved,
        })
    }
}

/// A release's major, minor, and patch numbers.
type Number = (u64, u64, u64);

fn parse_version(tag: &str) -> Option<Number> {
    tag.strip_prefix('v')
        .and_then(tenet_contracts::parse_version)
}

/// The result of merging local edits with an upstream change.
pub(super) struct Merge {
    pub conflicts: u32,
    pub text: String,
}

/// Three-way merge with `git merge-file --diff3`, labeling each side for the reader.
pub(super) fn merge(
    local: &str,
    base: &str,
    upstream: &str,
    labels: [&str; 3],
) -> Result<Merge, AdoptionError> {
    let directory = tempfile::tempdir().map_err(AdoptionError::Scratch)?;
    let paths = ["local", "base", "upstream"].map(|name| directory.path().join(name));
    for (path, text) in paths.iter().zip([local, base, upstream]) {
        fs::write(path, text).map_err(AdoptionError::Scratch)?;
    }
    let output = Command::new("git")
        .args(["merge-file", "-p", "--diff3"])
        .args(labels.iter().flat_map(|label| ["-L", label]))
        .args(&paths)
        .output()
        .map_err(GitError::Run)?;
    let conflicts = match output.status.code() {
        Some(code @ 0..=127) => code.unsigned_abs(),
        _ => {
            return Err(AdoptionError::MergeFailed(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        },
    };
    Ok(Merge {
        conflicts,
        text: String::from_utf8(output.stdout).map_err(AdoptionError::MergedNotUtf8)?,
    })
}
