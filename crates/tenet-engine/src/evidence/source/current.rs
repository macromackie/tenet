use alloc::sync::Arc;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt as _;
use std::{
    fs::{FileType, Metadata},
    io,
    path::Path,
    time::SystemTime,
};

use tokio::fs;

use super::{
    super::CaptureError,
    Side,
    reader::{ReadSource, read_stream},
};
use crate::{SourceError, Unreadable};

#[derive(Debug, Eq, PartialEq)]
struct MetadataIdentity {
    #[cfg(unix)]
    changed_nanoseconds: i64,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    device: u64,
    file_type: FileType,
    #[cfg(unix)]
    inode: u64,
    length: u64,
    modified: SystemTime,
}

impl MetadataIdentity {
    fn read(metadata: &Metadata) -> io::Result<Self> {
        Ok(Self {
            #[cfg(unix)]
            changed_nanoseconds: metadata.ctime_nsec(),
            #[cfg(unix)]
            changed_seconds: metadata.ctime(),
            #[cfg(unix)]
            device: metadata.dev(),
            file_type: metadata.file_type(),
            #[cfg(unix)]
            inode: metadata.ino(),
            length: metadata.len(),
            modified: metadata.modified()?,
        })
    }
}

fn is_stable(before: &Metadata, after: &Metadata) -> bool {
    let (Ok(before_identity), Ok(after_identity)) = (
        MetadataIdentity::read(before),
        MetadataIdentity::read(after),
    ) else {
        return false;
    };
    before_identity == after_identity
}

async fn capture_link(
    full: &Path,
    path: &Path,
    metadata: &Metadata,
) -> Result<ReadSource, SourceError> {
    let target = fs::read_link(full).await.map_err(|cause| {
        SourceError::Read {
            path: path.to_owned(),
            source: cause,
        }
    })?;
    let after = fs::symlink_metadata(full).await.map_err(|cause| {
        SourceError::Inspect {
            path: path.to_owned(),
            source: cause,
        }
    })?;
    if !is_stable(metadata, &after) {
        return Err(SourceError::ChangedDuringRead(path.to_owned()));
    }
    let bytes = target.as_os_str().as_encoded_bytes();
    Ok(ReadSource {
        hash: blake3::hash(bytes).to_hex().to_string(),
        is_utf8: false,
        line_count: 0,
        text: Err(Arc::new(CaptureError::Source(
            Unreadable::SymbolicLink.into(),
        ))),
    })
}

async fn capture_file(
    full: &Path,
    path: &Path,
    metadata: &Metadata,
    retain_bytes: usize,
) -> Result<ReadSource, SourceError> {
    let mut file = fs::File::open(full).await.map_err(|cause| {
        SourceError::Read {
            path: path.to_owned(),
            source: cause,
        }
    })?;
    let opened = file.metadata().await.map_err(|cause| {
        SourceError::Inspect {
            path: path.to_owned(),
            source: cause,
        }
    })?;
    if !is_stable(metadata, &opened) {
        return Err(SourceError::ChangedDuringRead(path.to_owned()));
    }
    let captured = read_stream(path, &mut file, metadata.len(), retain_bytes).await?;
    let after = file.metadata().await.map_err(|cause| {
        SourceError::Inspect {
            path: path.to_owned(),
            source: cause,
        }
    })?;
    let named = fs::symlink_metadata(full).await.map_err(|cause| {
        SourceError::Inspect {
            path: path.to_owned(),
            source: cause,
        }
    })?;
    if !is_stable(metadata, &after) || !is_stable(metadata, &named) {
        return Err(SourceError::ChangedDuringRead(path.to_owned()));
    }
    Ok(captured)
}

pub(super) async fn capture(
    root: &Path,
    path: &Path,
    retain_bytes: usize,
    allow_missing: bool,
) -> Result<Side, SourceError> {
    let full = root.join(path);
    let metadata = match fs::symlink_metadata(&full).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound && allow_missing => {
            return Ok(Side::Absent);
        },
        Err(cause) => {
            return Err(SourceError::Inspect {
                path: path.to_owned(),
                source: cause,
            });
        },
    };
    let captured = if metadata.file_type().is_symlink() {
        capture_link(&full, path, &metadata).await?
    } else if metadata.is_file() {
        capture_file(&full, path, &metadata, retain_bytes).await?
    } else {
        return Err(SourceError::NotRegularFile(path.to_owned()));
    };
    Ok(Side::Captured(captured))
}

pub(super) async fn read_current(
    root: &Path,
    path: &Path,
    retain_bytes: usize,
    allow_missing: bool,
) -> Side {
    match capture(root, path, retain_bytes, allow_missing).await {
        Ok(side) => side,
        Err(error) => Side::Unavailable(Arc::new(CaptureError::Source(error))),
    }
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    reason = "Snapshot metadata invariants are tested beside their owner."
)]
mod tests {
    #[cfg(unix)]
    use std::fs;

    #[cfg(unix)]
    use anyhow::Result;

    #[cfg(unix)]
    use super::MetadataIdentity;

    #[cfg(unix)]
    #[test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "A test reports its failure by panicking."
    )]
    fn change_time_difference_rejects_an_otherwise_identical_snapshot() -> Result<()> {
        let fixture = tempfile::tempdir()?;
        let path = fixture.path().join("file.rs");
        fs::write(&path, "before")?;
        let metadata = fs::metadata(path)?;
        let before = MetadataIdentity::read(&metadata)?;
        let mut after = MetadataIdentity::read(&metadata)?;
        assert_eq!(before, after);
        after.changed_nanoseconds = after.changed_nanoseconds.saturating_add(1);
        assert_ne!(before, after);
        Ok(())
    }
}
