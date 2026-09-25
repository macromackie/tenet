use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeSet,
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
    pub paths: BTreeSet<PathBuf>,
    pub base: String,
    pub broaden: bool,
}

pub(crate) fn changed(root: &Path, reference: &str) -> Result<Changes> {
    ensure!(
        !reference.starts_with('-'),
        "Git reference cannot start with '-'"
    );
    let top = String::from_utf8(output(root, &["rev-parse", "--show-toplevel"])?)?;
    let top = PathBuf::from(top.trim());
    let base = String::from_utf8(output(root, &["merge-base", "HEAD", reference])?)?
        .trim()
        .to_owned();
    let mut names = output(
        &top,
        &["diff", "--name-only", "--no-renames", "-z", &base, "--"],
    )?;
    names.extend(output(
        &top,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?);
    let mut paths = BTreeSet::new();
    let mut broaden = false;
    for name in names.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let path = top.join(std::str::from_utf8(name).context("Git path is not UTF-8")?);
        let ignore = path
            .file_name()
            .is_some_and(|s| s == ".gitignore" || s == ".ignore");
        if ignore && path.parent().is_some_and(|parent| root.starts_with(parent)) {
            broaden = true;
        }
        if let Ok(relative) = path.strip_prefix(root) {
            broaden |= ignore || relative.components().any(|p| p.as_os_str() == ".contracts");
            paths.insert(relative.to_owned());
        }
    }
    Ok(Changes {
        paths,
        base,
        broaden,
    })
}
