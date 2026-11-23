use std::fs;

use anyhow::Result;
use sha2::{Digest as _, Sha256};

use super::apply_audit_patches;
use crate::behavior::AuditFixPatch;

fn hash_text(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

#[test]
fn apply_audit_patches_writes_atomically_with_unified_diff() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let original = "def add(a, b):\n    return a - b\n";
    let updated = "def add(a, b):\n    return a + b\n";
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/math.py"), original)?;

    let changes = apply_audit_patches(
        root,
        &[AuditFixPatch {
            content: updated.to_owned(),
            original_hash: hash_text(original),
            path: String::from("src/math.py"),
        }],
    )?;
    anyhow::ensure!(changes.len() == 1, "expected 1 applied change");
    let first = changes
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing change"))?;
    anyhow::ensure!(first.path.as_os_str() == "src/math.py");
    anyhow::ensure!(
        first.patch.contains("-    return a - b") && first.patch.contains("+    return a + b"),
        "unexpected diff: {:?}",
        first.patch
    );
    anyhow::ensure!(fs::read_to_string(root.join("src/math.py"))? == updated);
    Ok(())
}

#[test]
fn apply_audit_patches_rejects_concurrent_edit_hash_mismatch() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let original = "export const value = 1;\n";
    let edited = "export const value = 99;\n";
    fs::write(root.join("app.ts"), edited)?;

    let result = apply_audit_patches(
        root,
        &[AuditFixPatch {
            content: String::from("export const value = 2;\n"),
            original_hash: hash_text(original),
            path: String::from("app.ts"),
        }],
    );
    anyhow::ensure!(result.is_err(), "expected hash mismatch rejection");
    anyhow::ensure!(
        fs::read_to_string(root.join("app.ts"))? == edited,
        "concurrent editor write must stay intact"
    );
    Ok(())
}

#[test]
fn apply_audit_patches_rejects_escape_and_protected_paths() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    fs::write(root.join("safe.ts"), "export const x = 1;\n")?;
    for bad_path in ["../escape.ts", "/etc/passwd", ".git/config", ".env"] {
        let result = apply_audit_patches(
            root,
            &[AuditFixPatch {
                content: String::from("bad\n"),
                original_hash: hash_text("bad\n"),
                path: String::from(bad_path),
            }],
        );
        anyhow::ensure!(result.is_err(), "expected path {bad_path:?} to be rejected");
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn apply_audit_patches_rejects_symlinks() -> Result<()> {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let content = "export const x = 1;\n";
    fs::write(root.join("real.ts"), content)?;
    symlink(root.join("real.ts"), root.join("link.ts"))?;

    let result = apply_audit_patches(
        root,
        &[AuditFixPatch {
            content: String::from("export const x = 2;\n"),
            original_hash: hash_text(content),
            path: String::from("link.ts"),
        }],
    );
    anyhow::ensure!(result.is_err(), "expected symlink patch target to fail");
    anyhow::ensure!(fs::read_to_string(root.join("real.ts"))? == content);
    Ok(())
}
