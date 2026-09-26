use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) fn output(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let result = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
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

pub(crate) fn changed(root: &Path, reference: &str) -> Result<Changes> {
    ensure!(
        !reference.starts_with('-'),
        "Git reference cannot start with '-'"
    );
    let top = top(root)?;
    let base = String::from_utf8(output(
        root,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )?)?
    .trim()
    .to_owned();
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
            paths.entry(path.to_owned()).or_insert(None);
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
    ensure!(
        listing.starts_with(b"100644 ") || listing.starts_with(b"100755 "),
        "base input must be a regular file: {relative}"
    );
    let object = format!("{base}:{relative}");
    let size: usize = String::from_utf8(output(&top, &["cat-file", "-s", &object])?)?
        .trim()
        .parse()?;
    ensure!(
        size <= ev_grep_core::MAX_FILE_BYTES,
        "base file exceeds 64 KiB: {relative}"
    );
    let bytes = output(&top, &["cat-file", "blob", &object])?;
    ensure!(!bytes.contains(&0), "binary base file: {relative}");
    Ok(Some(
        String::from_utf8(bytes).context("base file is not UTF-8")?,
    ))
}
