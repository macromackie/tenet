use std::{
    path::Path,
    process::{Command, Output},
};

use anyhow::Result;

/// Git with fixed commit identity and dates, so fixture revisions depend only on their files.
#[expect(
    clippy::panic_in_result_fn,
    reason = "A fixture command failure must stop its owning test."
)]
pub(super) fn run_git(root: &Path, args: &[&str]) -> Result<Output> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
        ])
        .args(args)
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .output()?;
    assert!(
        output.status.success(),
        "git {args:?}: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}
