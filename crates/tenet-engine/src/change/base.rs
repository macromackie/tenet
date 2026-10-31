use std::path::Path;

use super::{SourceError, Unreadable, check_capture_size, decode};
use crate::git;

pub(crate) fn read_before(
    root: &Path,
    base: &str,
    path: &Path,
) -> Result<Option<String>, SourceError> {
    let top = git::find_top(root)?;
    let full = root.join(path);
    let relative = full
        .strip_prefix(&top)?
        .to_str()
        .ok_or(SourceError::NotUtf8Path)?;
    let listing = git::run_git(&top, &["ls-tree", "-z", base, "--", relative])?;
    if listing.is_empty() {
        return Ok(None);
    }
    if listing.starts_with(b"120000 ") {
        return Err(Unreadable::SymbolicLink.into());
    }
    if !(listing.starts_with(b"100644 ") || listing.starts_with(b"100755 ")) {
        return Err(SourceError::BaseNotRegularFile(relative.to_owned()));
    }
    let object = format!("{base}:{relative}");
    let size = String::from_utf8(git::run_git(&top, &["cat-file", "-s", &object])?)?
        .trim()
        .parse()?;
    check_capture_size(path, size)?;
    let bytes = git::run_git(&top, &["cat-file", "blob", &object])?;
    Ok(Some(decode(bytes)?))
}
