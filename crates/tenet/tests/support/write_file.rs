use std::{fs, path::Path};

use anyhow::Result;

/// Write a named fixture file after creating only its parent folders.
pub(super) fn write_file(root: &Path, relative: &str, content: &str) -> Result<()> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(fs::write(path, content)?)
}
