use std::{fs, path::Path};

use anyhow::{Context as _, Result};
use serde_json::json;

use super::{
    build_tenet::build_tenet,
    commit::commit,
    json_lines::read_json_lines,
    run_git::run_git,
};

fn read_head(root: &Path) -> Result<String> {
    Ok(
        String::from_utf8(run_git(root, &["rev-parse", "HEAD"])?.stdout)?
            .trim()
            .to_owned(),
    )
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A test reports its failure by panicking."
)]
#[expect(
    clippy::too_many_lines,
    reason = "A head, target and test merge exercise the same pull request."
)]
fn pull_request_review_pins_its_base_and_rechecks_changed_rules() -> Result<()> {
    let repository = tempfile::tempdir()?;
    let root = repository.path();
    let event = tempfile::NamedTempFile::new()?;
    let contract = root.join(".contracts/001-errors/CONTRACT.md");
    fs::create_dir_all(contract.parent().context("contract directory")?)?;
    let rule = "---\nname: errors\nmessage: Preserve errors.\n---\n## Rules\nPropagate errors.\n";
    fs::write(&contract, rule)?;
    fs::write(root.join("source.txt"), "original\n")?;
    drop(run_git(root, &["init", "-qb", "main"])?);
    commit(root, "base")?;
    let ancestor = read_head(root)?;

    drop(run_git(root, &["checkout", "-qb", "feature"])?);
    fs::write(root.join("source.txt"), "changed\n")?;
    commit(root, "feature")?;
    let feature = read_head(root)?;
    drop(run_git(root, &["checkout", "-q", "main"])?);
    fs::write(root.join("other.txt"), "other work\n")?;
    commit(root, "advance target")?;
    let target = read_head(root)?;
    fs::write(
        event.path(),
        serde_json::to_vec(
            &json!({"pull_request": {"head": {"sha":feature}, "base": {"sha":target}}}),
        )?,
    )?;

    let run = |args: &[&str]| -> Result<_> {
        Ok(build_tenet()
            .arg("--root")
            .arg(root)
            .args(["check", "--no-model"])
            .args(args)
            .env("GITHUB_ACTIONS", "true")
            .env("GITHUB_EVENT_NAME", "pull_request")
            .env("GITHUB_EVENT_PATH", event.path())
            .env("GITHUB_WORKSPACE", root)
            .env_remove("TENET_STATE_DIR")
            .output()?)
    };
    let read_base = |args: &[&str]| -> Result<Option<String>> {
        let output = run(args)?;
        let events = read_json_lines(&output.stdout)?;
        let begin = events
            .first()
            .with_context(|| String::from_utf8_lossy(&output.stderr).into_owned())?;
        Ok(begin
            .pointer("/run/base")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned))
    };

    assert_eq!(run(&[])?.status.code(), Some(2_i32));
    assert_eq!(
        read_base(&["--base", &feature, "--json"])?.as_ref(),
        Some(&feature)
    );
    drop(run_git(
        root,
        &["merge", "--no-ff", "--no-edit", "feature"],
    )?);
    assert_eq!(read_base(&["--json"])?.as_ref(), Some(&target));

    drop(run_git(root, &["checkout", "-q", "feature"])?);
    assert_eq!(read_base(&["--json"])?.as_ref(), Some(&ancestor));
    let review = String::from_utf8(run(&[])?.stderr)?;
    assert!(review.contains("REVIEW  Preserve errors."), "{review:?}");
    assert!(review.contains("1 for review"), "{review:?}");
    assert!(!review.contains("Combined review"), "{review:?}");

    fs::write(&contract, format!("{rule}Preserve the original cause.\n"))?;
    assert!(read_base(&["--json"])?.is_none());
    assert_eq!(
        read_base(&["--base", &ancestor, "--json"])?.as_ref(),
        Some(&ancestor)
    );
    Ok(())
}
