use std::path::Path;

use anyhow::Result;

use crate::run_git::run_git;

/// Commit the fixture's complete current contents under its declared message.
pub(super) fn commit(root: &Path, message: &str) -> Result<()> {
    drop(run_git(root, &["add", "-A"])?);
    drop(run_git(root, &["commit", "-qm", message])?);
    Ok(())
}
