use std::{fs, path::PathBuf, process::Command};

use anyhow::{Context, Result, bail, ensure};
use tempfile::TempDir;

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

    /// Read `contracts/NAME/CONTRACT.md` at a commit; `None` when the library has no such contract.
    pub(super) fn contract(&self, commit: &str, name: &str) -> Result<Option<String>> {
        let path = format!("contracts/{name}/CONTRACT.md");
        let listing = git::output(&self.repository, &["ls-tree", commit, "--", &path])?;
        if listing.is_empty() {
            return Ok(None);
        }
        ensure!(
            listing.starts_with(b"100644 ") || listing.starts_with(b"100755 "),
            "{path} must be a regular file in {}",
            self.url
        );
        let bytes = git::output(
            &self.repository,
            &["cat-file", "blob", &format!("{commit}:{path}")],
        )?;
        Ok(Some(String::from_utf8(bytes).with_context(|| {
            format!("{path} is not UTF-8 in {}", self.url)
        })?))
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
    let mut parts = tag.strip_prefix('v')?.split('.');
    let number = |part: Option<&str>| {
        part.filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))?
            .parse()
            .ok()
    };
    let version = (
        number(parts.next())?,
        number(parts.next())?,
        number(parts.next())?,
    );
    parts.next().is_none().then_some(version)
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
        text: String::from_utf8(output.stdout).context("merged contract is not UTF-8")?,
        conflicts,
    })
}
