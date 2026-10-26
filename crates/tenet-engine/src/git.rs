use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::BTreeMap,
    io::{Seek, Write},
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) fn output(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let result = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .context("cannot run Git")?;
    ensure!(
        result.status.success(),
        "Git {} failed: {}",
        args.first().copied().unwrap_or("command"),
        String::from_utf8_lossy(&result.stderr).trim()
    );
    Ok(result.stdout)
}

/// A path Git ignores and the rule that ignores it, as `SOURCE:LINE:PATTERN`.
pub(crate) struct Ignored {
    pub path: String,
    pub rule: String,
}

/// The first of `paths` that the project's ignore rules exclude; tracked files are never ignored.
pub(crate) fn ignored(root: &Path, paths: &[PathBuf]) -> Result<Option<Ignored>> {
    // Without `--verbose`, Git lists only excluded paths; with it, Git also lists paths that a
    // negated `!PATTERN` keeps. So find the first excluded path, then ask for the rule that decided it.
    let excluded = check_ignore(root, false, paths)?;
    let Some(path) = excluded.first() else {
        return Ok(None);
    };
    let fields = check_ignore(root, true, &[PathBuf::from(path)])?;
    let [source, line, pattern, path] = fields.as_slice() else {
        bail!("malformed Git check-ignore output");
    };
    Ok(Some(Ignored {
        path: path.clone(),
        rule: format!("{source}:{line}:{pattern}"),
    }))
}

/// Run `git check-ignore` on `paths` and return its NUL-separated output fields; none when Git
/// excludes none of them.
fn check_ignore(root: &Path, verbose: bool, paths: &[PathBuf]) -> Result<Vec<String>> {
    // Paths go through a file on standard input, since `-z` needs `--stdin`.
    let mut input = tempfile::tempfile()?;
    for path in paths {
        input.write_all(path.to_str().context("path is not UTF-8")?.as_bytes())?;
        input.write_all(b"\0")?;
    }
    input.rewind()?;
    let result = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "--stdin", "-z"])
        .args(verbose.then_some("--verbose"))
        .stdin(input)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .context("cannot run Git")?;
    match result.status.code() {
        Some(0) => Ok(String::from_utf8(result.stdout)?
            .split_terminator('\0')
            .map(str::to_owned)
            .collect()),
        Some(1) => Ok(Vec::new()),
        _ => bail!(
            "Git check-ignore failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        ),
    }
}

pub(crate) fn head(root: &Path) -> Option<String> {
    output(root, &["rev-parse", "HEAD"])
        .ok()
        .and_then(|s| String::from_utf8(s).ok())
        .map(|s| s.trim().to_owned())
}

pub(crate) struct Changes {
    pub paths: BTreeMap<PathBuf, Option<PathBuf>>,
    pub base: String,
}

/// The commit `reference` names.
pub(crate) fn commit(root: &Path, reference: &str) -> Result<String> {
    ensure!(
        !reference.starts_with('-'),
        "Git reference cannot start with '-'"
    );
    let oid = output(
        root,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )
    .with_context(|| {
        format!("base {reference} is not a commit in this repository; fetch it or check the name")
    })?;
    Ok(String::from_utf8(oid)?.trim().to_owned())
}

pub(crate) fn changed(root: &Path, reference: &str, discovered: &[PathBuf]) -> Result<Changes> {
    let base = commit(root, reference)?;
    let top = top(root)?;
    let names = output(
        &top,
        &["diff", "--name-status", "--find-renames", "-z", &base, "--"],
    )?;
    let mut entries = names.split(|b| *b == 0).filter(|s| !s.is_empty());
    let mut paths = BTreeMap::new();
    while let Some(status) = entries.next() {
        let first = entries.next().context("missing Git path")?;
        let first = top.join(std::str::from_utf8(first)?);
        let second = if status.starts_with(b"R") {
            Some(top.join(std::str::from_utf8(
                entries.next().context("missing rename path")?,
            )?))
        } else {
            None
        };
        if let Some(second) = second {
            match (first.strip_prefix(root), second.strip_prefix(root)) {
                (Ok(old), Ok(new)) => {
                    paths.insert(new.to_owned(), Some(old.to_owned()));
                }
                (Ok(old), Err(_)) => {
                    paths.insert(old.to_owned(), None);
                }
                (Err(_), Ok(new)) => {
                    paths.insert(new.to_owned(), None);
                }
                _ => {}
            }
        } else if let Ok(path) = first.strip_prefix(root) {
            paths.insert(path.to_owned(), None);
        }
    }
    for name in output(&top, &["ls-files", "--others", "--exclude-standard", "-z"])?
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
    {
        if let Ok(path) = top.join(std::str::from_utf8(name)?).strip_prefix(root) {
            let start = discovered.partition_point(|candidate| candidate.as_path() < path);
            for candidate in discovered[start..]
                .iter()
                .take_while(|candidate| candidate.starts_with(path))
            {
                paths.entry(candidate.clone()).or_insert(None);
            }
        }
    }
    Ok(Changes { paths, base })
}

fn top(root: &Path) -> Result<PathBuf> {
    Ok(PathBuf::from(
        String::from_utf8(output(root, &["rev-parse", "--show-toplevel"])?)?.trim(),
    ))
}

pub(crate) fn before(root: &Path, base: &str, path: &Path) -> Result<Option<String>> {
    let top = top(root)?;
    let full = root.join(path);
    let relative = full
        .strip_prefix(&top)?
        .to_str()
        .context("path is not UTF-8")?;
    let listing = output(&top, &["ls-tree", "-z", base, "--", relative])?;
    if listing.is_empty() {
        return Ok(None);
    }
    if listing.starts_with(b"120000 ") {
        return Err(crate::change::Unreadable::SymbolicLink.into());
    }
    ensure!(
        listing.starts_with(b"100644 ") || listing.starts_with(b"100755 "),
        "base input must be a regular file: {relative}"
    );
    let object = format!("{base}:{relative}");
    let size = String::from_utf8(output(&top, &["cat-file", "-s", &object])?)?
        .trim()
        .parse()?;
    crate::change::check_capture_size(path, size)?;
    crate::change::decode(output(&top, &["cat-file", "blob", &object])?).map(Some)
}

pub(crate) fn branch(root: &Path) -> Option<String> {
    output(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .and_then(|s| String::from_utf8(s).ok())
        .map(|s| s.trim().to_owned())
        .filter(|name| !name.is_empty())
}
