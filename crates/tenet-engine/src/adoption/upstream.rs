use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail, ensure};
use tempfile::TempDir;
use tenet_contracts::is_contract_asset;

use super::Kind;
use crate::git;

/// A temporary bare clone of a contract library. Nothing persists after the command.
pub(super) struct Upstream {
    repository: PathBuf,
    url: String,
    _directory: TempDir,
}

/// A requested reference and the commit it resolved to.
pub(super) struct Revision {
    pub reference: String,
    pub commit: String,
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
    pub(super) fn clone(url: &str) -> Result<Self> {
        ensure!(!url.starts_with('-'), "source URL cannot start with '-'");
        let directory = tempfile::tempdir()?;
        git::output(
            directory.path(),
            &["clone", "--bare", "--quiet", "--", url, "library.git"],
        )
        .with_context(|| format!("cannot fetch contract library {url}"))?;
        Ok(Self {
            repository: directory.path().join("library.git"),
            url: url.into(),
            _directory: directory,
        })
    }

    /// Resolve a tag, branch, or commit; without one, take the highest `vX.Y.Z` tag.
    pub(super) fn resolve(&self, reference: Option<&str>) -> Result<Revision> {
        let reference = match reference {
            Some(reference) => reference.to_owned(),
            None => self.latest_tag()?,
        };
        ensure!(
            !reference.starts_with('-'),
            "Git reference cannot start with '-'"
        );
        let commit = git::output(
            &self.repository,
            &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
        )
        .with_context(|| format!("{reference} does not exist in {}", self.url))?;
        Ok(Revision {
            reference,
            commit: String::from_utf8(commit)?.trim().to_owned(),
        })
    }

    /// Move a version tag to the highest version tag; a branch or commit resolves to itself.
    pub(super) fn newest(&self, locked: &str) -> Result<Revision> {
        if version(locked).is_some() {
            self.resolve(None)
        } else {
            self.resolve(Some(locked))
        }
    }

    /// Read every file of `contracts/NAME/` or `skills/NAME/` at a commit. `None` when the folder
    /// has no CONTRACT.md or SKILL.md. Refuses a contract file other than CONTRACT.md and
    /// `assets/<tool>/` files, which this Tenet cannot install whole.
    pub(super) fn folder(&self, commit: &str, kind: Kind, name: &str) -> Result<Option<Files>> {
        let prefix = format!("{}/{name}/", kind.name());
        let listing = git::output(
            &self.repository,
            &["ls-tree", "-r", "-z", commit, "--", &prefix],
        )?;
        let mut files = Files::new();
        for entry in listing.split(|b| *b == 0).filter(|entry| !entry.is_empty()) {
            let entry = std::str::from_utf8(entry).context("library path is not UTF-8")?;
            let (meta, path) = entry.split_once('\t').context("malformed Git listing")?;
            let Some(relative) = path.strip_prefix(&prefix).map(PathBuf::from) else {
                continue;
            };
            ensure!(
                kind == Kind::Skills
                    || relative == Path::new(kind.document())
                    || is_contract_asset(&relative),
                "{path} in {} is neither CONTRACT.md nor a file under assets/<tool>/, so Tenet {} \
                 cannot install it; the contract may need a newer Tenet",
                self.url,
                env!("CARGO_PKG_VERSION")
            );
            let mut fields = meta.split(' ');
            let executable = match fields.next() {
                Some("100644") => false,
                Some("100755") => true,
                _ => bail!("{path} must be a regular file in {}", self.url),
            };
            let object = fields.nth(1).context("malformed Git listing")?;
            let bytes = git::output(&self.repository, &["cat-file", "blob", object])?;
            files.insert(relative, File { bytes, executable });
        }
        Ok(files
            .contains_key(Path::new(kind.document()))
            .then_some(files))
    }

    fn latest_tag(&self) -> Result<String> {
        let tags = String::from_utf8(git::output(
            &self.repository,
            &["for-each-ref", "--format=%(refname:short)", "refs/tags"],
        )?)?;
        tags.lines()
            .filter_map(|tag| version(tag).map(|number| (number, tag)))
            .max()
            .map(|(_, tag)| tag.to_owned())
            .with_context(|| {
                format!(
                    "{} has no vX.Y.Z tags; name a reference with SOURCE/NAME@REF",
                    self.url
                )
            })
    }
}

fn version(tag: &str) -> Option<(u64, u64, u64)> {
    tag.strip_prefix('v')
        .and_then(tenet_contracts::parse_version)
}

/// The result of merging local edits with an upstream change.
pub(super) struct Merge {
    pub text: String,
    pub conflicts: u32,
}

/// Three-way merge with `git merge-file --diff3`, labelling each side for the reader.
pub(super) fn merge(local: &str, base: &str, upstream: &str, labels: [&str; 3]) -> Result<Merge> {
    let directory = tempfile::tempdir()?;
    let paths = ["local", "base", "upstream"].map(|name| directory.path().join(name));
    for (path, text) in paths.iter().zip([local, base, upstream]) {
        fs::write(path, text)?;
    }
    let output = Command::new("git")
        .args(["merge-file", "-p", "--diff3"])
        .args(labels.iter().flat_map(|label| ["-L", label]))
        .args(&paths)
        .output()
        .context("cannot run Git")?;
    let conflicts = match output.status.code() {
        Some(code @ 0..=127) => u32::try_from(code)?,
        _ => bail!(
            "Git merge-file failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    };
    Ok(Merge {
        text: String::from_utf8(output.stdout).context("merged document is not UTF-8")?,
        conflicts,
    })
}
