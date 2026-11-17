use std::fs;

use anyhow::Result;

pub(super) fn create_fixture(files: &[(&str, &str)]) -> Result<tempfile::TempDir> {
    let root = tempfile::tempdir()?;
    fs::create_dir_all(root.path().join(".contracts/001-errors"))?;
    fs::write(
        root.path().join(".contracts/001-errors/CONTRACT.md"),
        "---\nname: errors\nmessage: Preserve failures.\n---\n## Rules\nPropagate failures.\n",
    )?;
    for (name, contents) in files {
        fs::write(root.path().join(name), contents)?;
    }
    Ok(root)
}
