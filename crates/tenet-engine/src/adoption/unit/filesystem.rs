//! Installed file reads, writes, and deletion retain their project-relative identities.

use alloc::collections::BTreeSet;
use core::{cmp::Reverse, str};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use super::{AdoptionError, File, Unit};

/// Read a Markdown file's bytes as text.
pub(in crate::adoption) fn decode_text<'text>(
    bytes: &'text [u8],
    path: &Path,
) -> Result<&'text str, AdoptionError> {
    str::from_utf8(bytes).map_err(|source| {
        AdoptionError::NotUtf8Text {
            path: path.to_owned(),
            source,
        }
    })
}

/// Write one installed file, creating its folders; `path` is relative to the project root.
pub(in crate::adoption) fn write(
    root: &Path,
    path: &Path,
    file: &File,
) -> Result<(), AdoptionError> {
    let full = root.join(path);
    let cannot = |source| {
        AdoptionError::Write {
            path: path.to_owned(),
            source,
        }
    };
    fs::create_dir_all(full.parent().ok_or(AdoptionError::NoInstalledFolder)?).map_err(cannot)?;
    fs::write(&full, &file.bytes).map_err(cannot)?;
    #[cfg(unix)]
    if file.executable {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&full, fs::Permissions::from_mode(0o755)).map_err(cannot)?;
    }
    Ok(())
}

/// Read one installed file; `path` is relative to the project root.
pub(in crate::adoption) fn read(root: &Path, path: &Path) -> Result<Vec<u8>, AdoptionError> {
    fs::read(root.join(path)).map_err(|source| {
        AdoptionError::Read {
            path: path.to_owned(),
            source,
        }
    })
}

/// The first existing path that keeps a new file from being written at `folder/path`: anything
/// at that path, or a file or symlink where a folder between `folder` and that path belongs.
///
/// `folder` is relative to the project root, and `path` to `folder`.
pub(in crate::adoption) fn find_obstruction(
    root: &Path,
    folder: &Path,
    path: &Path,
) -> Result<Option<PathBuf>, AdoptionError> {
    let target = folder.join(path);
    let mut current = folder.to_owned();
    for part in path.components() {
        current.push(part);
        let metadata = match root.join(&current).symlink_metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(AdoptionError::Inspect {
                    path: current,
                    source,
                });
            },
        };
        if current == target || !metadata.is_dir() {
            return Ok(Some(current));
        }
    }
    Ok(None)
}

impl Unit {
    /// Delete the locked files, then each folder they leave empty, up to and including the copy's
    /// folder. Files the lock does not list stay.
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    pub(in crate::adoption) fn delete(&self, root: &Path) -> Result<(), AdoptionError> {
        let folder = root.join(&self.folder);
        let mut directories = BTreeSet::new();
        for path in self.files.keys() {
            let full = folder.join(path);
            if full.exists() {
                fs::remove_file(&full).map_err(|source| {
                    AdoptionError::Remove {
                        path: self.folder.join(path),
                        source,
                    }
                })?;
            }
            directories.extend(
                full.ancestors()
                    .skip(1)
                    .take_while(|directory| directory.starts_with(&folder))
                    .map(Path::to_owned),
            );
        }
        let mut sorted: Vec<_> = directories.into_iter().collect();
        sorted.sort_by_key(|directory| Reverse(directory.components().count()));
        for directory in sorted {
            let cannot = |source| {
                AdoptionError::Remove {
                    path: directory.clone(),
                    source,
                }
            };
            if directory.is_dir() && directory.read_dir().map_err(cannot)?.next().is_none() {
                fs::remove_dir(&directory).map_err(cannot)?;
            }
        }
        Ok(())
    }
}
