use alloc::{collections::BTreeMap, string::FromUtf8Error};
use core::{str, str::Utf8Error};
use std::{
    io,
    io::{Seek as _, Write as _},
    path::{Path, PathBuf},
    process::Command,
};

/// Why a Git command failed or its output cannot be read.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("Git check-ignore failed: {0:#}")]
    CheckIgnore(String),
    #[error("Git {command:?} failed: {stderr:#}")]
    Failed { command: String, stderr: String },
    #[error(transparent)]
    Input(io::Error),
    #[error("malformed Git check-ignore output")]
    MalformedCheckIgnore,
    #[error("missing Git path")]
    MissingPath,
    #[error("missing rename path")]
    MissingRenamePath,
    #[error("path is not UTF-8")]
    NotUtf8Path,
    #[error("Git reference cannot start with '-'")]
    OptionReference,
    #[error(transparent)]
    OutputText(#[from] FromUtf8Error),
    #[error(transparent)]
    PathText(#[from] Utf8Error),
    #[error("cannot run Git")]
    Run(#[source] io::Error),
    #[error("base {reference:?} is not a commit in this repository; fetch it or check the name")]
    UnknownBase {
        reference: String,
        source: Box<Self>,
    },
}

pub(crate) fn run_git(root: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
    let result = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(GitError::Run)?;
    if !result.status.success() {
        return Err(GitError::Failed {
            command: args.first().copied().unwrap_or("command").to_owned(),
            stderr: String::from_utf8_lossy(&result.stderr).trim().to_owned(),
        });
    }
    Ok(result.stdout)
}

/// A path Git ignores and the rule that ignores it, as `SOURCE:LINE:PATTERN`.
pub(crate) struct Ignored {
    pub path: String,
    pub rule: String,
}

/// The first of `paths` that the project's ignore rules exclude; tracked files are never ignored.
pub(crate) fn find_ignored(root: &Path, paths: &[PathBuf]) -> Result<Option<Ignored>, GitError> {
    // Without `--verbose`, Git lists only excluded paths; with it, Git also lists paths that a
    // negated `!PATTERN` keeps. So find the first excluded path, then ask for the rule that decided
    // it.
    let excluded = check_ignore(root, false, paths)?;
    let Some(path) = excluded.first() else {
        return Ok(None);
    };
    let fields = check_ignore(root, true, &[PathBuf::from(path)])?;
    let [source, line, pattern, matched] = fields.as_slice() else {
        return Err(GitError::MalformedCheckIgnore);
    };
    Ok(Some(Ignored {
        path: matched.clone(),
        rule: format!("{source}:{line}:{pattern}"),
    }))
}

/// Run `git check-ignore` on `paths` and return its NUL-separated output fields; none when Git
/// excludes none of them.
fn check_ignore(root: &Path, verbose: bool, paths: &[PathBuf]) -> Result<Vec<String>, GitError> {
    // Paths go through a file on standard input, since `-z` needs `--stdin`.
    let mut input = tempfile::tempfile().map_err(GitError::Input)?;
    for path in paths {
        let text = path.to_str().ok_or(GitError::NotUtf8Path)?;
        input.write_all(text.as_bytes()).map_err(GitError::Input)?;
        input.write_all(b"\0").map_err(GitError::Input)?;
    }
    input.rewind().map_err(GitError::Input)?;
    let result = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "--stdin", "-z"])
        .args(verbose.then_some("--verbose"))
        .stdin(input)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(GitError::Run)?;
    match result.status.code() {
        Some(0) => {
            Ok(String::from_utf8(result.stdout)?
                .split_terminator('\0')
                .map(str::to_owned)
                .collect())
        },
        Some(1) => Ok(Vec::new()),
        _ => {
            Err(GitError::CheckIgnore(
                String::from_utf8_lossy(&result.stderr).trim().to_owned(),
            ))
        },
    }
}

pub(crate) fn read_head(root: &Path) -> Option<String> {
    run_git(root, &["rev-parse", "HEAD"])
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|text| text.trim().to_owned())
}

pub(crate) struct Changes {
    pub base: String,
    pub paths: BTreeMap<PathBuf, Option<PathBuf>>,
}

/// The commit `reference` names.
pub(crate) fn commit(root: &Path, reference: &str) -> Result<String, GitError> {
    if reference.starts_with('-') {
        return Err(GitError::OptionReference);
    }
    let oid = run_git(
        root,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )
    .map_err(|source| {
        GitError::UnknownBase {
            reference: reference.to_owned(),
            source: Box::new(source),
        }
    })?;
    Ok(String::from_utf8(oid)?.trim().to_owned())
}

#[expect(
    clippy::indexing_slicing,
    reason = "`partition_point` returns an index no larger than the slice length."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
#[expect(
    clippy::else_if_without_else,
    reason = "`needless_else` rejects the empty `else` this lint asks for."
)]
pub(crate) fn list_changed(
    root: &Path,
    reference: &str,
    discovered: &[PathBuf],
) -> Result<Changes, GitError> {
    let base = commit(root, reference)?;
    let top = find_top(root)?;
    let names = run_git(
        &top,
        &["diff", "--name-status", "--find-renames", "-z", &base, "--"],
    )?;
    let mut entries = names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty());
    let mut paths = BTreeMap::new();
    while let Some(status) = entries.next() {
        let first = entries.next().ok_or(GitError::MissingPath)?;
        let first_path = top.join(str::from_utf8(first)?);
        let second = if status.starts_with(b"R") {
            Some(top.join(str::from_utf8(
                entries.next().ok_or(GitError::MissingRenamePath)?,
            )?))
        } else {
            None
        };
        if let Some(second_path) = second {
            match (
                first_path.strip_prefix(root),
                second_path.strip_prefix(root),
            ) {
                (Ok(old), Ok(new)) => {
                    drop(paths.insert(new.to_owned(), Some(old.to_owned())));
                },
                (Ok(old), Err(_)) => {
                    drop(paths.insert(old.to_owned(), None));
                },
                (Err(_), Ok(new)) => {
                    drop(paths.insert(new.to_owned(), None));
                },
                _ => {},
            }
        } else if let Ok(path) = first_path.strip_prefix(root) {
            drop(paths.insert(path.to_owned(), None));
        }
    }
    for name in run_git(&top, &["ls-files", "--others", "--exclude-standard", "-z"])?
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        if let Ok(path) = top.join(str::from_utf8(name)?).strip_prefix(root) {
            let start = discovered.partition_point(|candidate| candidate.as_path() < path);
            for candidate in discovered[start..]
                .iter()
                .take_while(|candidate| candidate.starts_with(path))
            {
                let _: &mut Option<PathBuf> = paths.entry(candidate.clone()).or_insert(None);
            }
        }
    }
    Ok(Changes { base, paths })
}

pub(crate) fn find_top(root: &Path) -> Result<PathBuf, GitError> {
    Ok(PathBuf::from(
        String::from_utf8(run_git(root, &["rev-parse", "--show-toplevel"])?)?.trim(),
    ))
}

pub(crate) fn read_branch(root: &Path) -> Option<String> {
    run_git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|text| text.trim().to_owned())
        .filter(|name| !name.is_empty())
}
