//! Native fixes are restricted to the installed asset and the requested Git change.
#![cfg(unix)]

#[path = "support/build_tenet.rs"]
mod build_tenet;
#[path = "support/commit.rs"]
mod commit;
#[path = "support/run_git.rs"]
mod run_git;
#[path = "support/write_file.rs"]
mod write_file;

use std::{
    env,
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result};
use build_tenet::build_tenet;
use commit::commit;
use run_git::run_git;
use serde_json::{Value, json};
use tempfile::tempdir;
use tenet_contracts::{hash_contract, hash_file};
use write_file::write_file;

const CONTRACT: &str = "---\nname: spacing\nmessage: Remove trailing spaces.\n---\n\n## \
                        Rules\n\nRemove trailing spaces.\n";
const DOCUMENT: &str = ".contracts/001-spacing/CONTRACT.md";
const ASSET: &str = "assets/ruff/ruff.toml";
const CONFIG: &str = "[lint]\nselect = ['W291']\n";

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A failed check should identify the observed CLI outcome."
)]
fn fixes_only_governed_changes_and_refuses_modified_assets() -> Result<()> {
    let project = tempdir()?;
    let tools = tempdir()?;
    let root = project.path();
    drop(run_git(root, &["init", "-q"])?);
    let asset = install_fixture(root)?;
    write_file(root, "untouched.py", "original  \n")?;
    commit(root, "baseline")?;
    write_file(root, "added.py", "new  \n")?;
    let path = build_native_tool_path(tools.path())?;
    let run = || {
        build_tenet()
            .arg("--root")
            .arg(root)
            .args(["fix", "--base", "HEAD", "--json"])
            .env("PATH", &path)
            .output()
    };
    let result = run()?;
    assert!(
        result.status.success(),
        "{:?}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout)?;
    assert_eq!(report.pointer("/checks/0/passed"), Some(&Value::Bool(true)));
    assert_eq!(report.pointer("/changes/0/path"), Some(&json!("added.py")));
    assert_eq!(
        fs::read_to_string(root.join("untouched.py"))?,
        "original  \n"
    );
    assert_eq!(fs::read_to_string(root.join("added.py"))?, "new\n");
    assert!(!root.join("stray.py").exists());
    fs::write(
        root.join(DOCUMENT),
        format!("{CONTRACT}\n## Local adaptations\n\nAllow trailing spaces in fixtures.\n"),
    )?;
    write_file(root, "added.py", "fixture  \n")?;
    let adapted: Value = serde_json::from_slice(&run()?.stdout)?;
    assert_eq!(adapted.get("manual"), Some(&json!(["spacing"])));
    assert_eq!(fs::read_to_string(root.join("added.py"))?, "fixture  \n");
    fs::write(root.join(DOCUMENT), CONTRACT)?;
    fs::write(root.join(asset), "[lint]\nselect = ['ALL']\n")?;
    assert!(!run()?.status.success());
    Ok(())
}

fn install_fixture(root: &Path) -> Result<PathBuf> {
    write_file(root, DOCUMENT, CONTRACT)?;
    let asset = Path::new(DOCUMENT).with_file_name("assets/ruff/ruff.toml");
    fs::create_dir_all(root.join(&asset).parent().context("missing asset folder")?)?;
    fs::write(root.join(&asset), CONFIG)?;
    let lock = json!({
        "version": 2_u32, "sources": {"tenet": "https://example.invalid/contracts"},
        "contracts": {(DOCUMENT): {
            "source":"tenet", "contract":"spacing", "ref":"v1.0.0", "commit":"a".repeat(40),
            "hash":hash_contract(CONTRACT)?, "files": {(ASSET): hash_file(CONFIG.as_bytes())}
        }}
    });
    fs::write(root.join(".tenet.lock"), serde_json::to_vec(&lock)?)?;
    Ok(asset)
}

fn build_native_tool_path(tools: &Path) -> Result<OsString> {
    // This native-tool stand-in verifies rerun ordering and also creates an unrelated file.
    // Only the selected file may be copied back to the project.
    let executable = tools.join("ruff");
    fs::write(
        &executable,
        "#!/bin/sh\nset -eu\ncase \" $* \" in *untouched.py*) exit 5;; esac\ncase \" $* \" in\n \
         *' --fix '*) printf 'new\\n' > added.py; touch stray.py;;\n *) test \"$(cat added.py)\" \
         = new;;\nesac\nprintf '[]'\n",
    )?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
    let environment = env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![tools.to_path_buf()];
    paths.extend(env::split_paths(&environment));
    Ok(env::join_paths(paths)?)
}
