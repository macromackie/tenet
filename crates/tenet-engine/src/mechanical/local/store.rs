use std::{
    fs::{self, File},
    io::{self, Read as _, Write as _},
    path::Path,
};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use super::{Observation, Snapshot};

const MAX_RECORD_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub observation: Observation,
    pub snapshot: Option<Snapshot>,
    pub version: u8,
}

impl Record {
    pub(super) const fn new(observation: Observation, snapshot: Option<Snapshot>) -> Self {
        Self {
            observation,
            snapshot,
            version: 1,
        }
    }

    pub(super) fn read(path: &Path) -> io::Result<Option<Self>> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if !file.metadata()?.is_file() || file.metadata()?.len() > MAX_RECORD_BYTES {
            return Err(io::Error::other(
                "invalid local obligation record size or type",
            ));
        }
        let mut bytes = Vec::new();
        let _count = file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_RECORD_BYTES {
            return Err(io::Error::other("local obligation record exceeds 2 MiB"));
        }
        let record: Self = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if record.version != 1 {
            return Err(io::Error::other(
                "unsupported local obligation record version",
            ));
        }
        Ok(Some(record))
    }

    pub(super) fn write(&self, path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("missing state directory"))?;
        let mut temp = NamedTempFile::new_in(parent)?;
        serde_json::to_writer(&mut temp, self).map_err(io::Error::other)?;
        temp.flush()?;
        if temp.as_file().metadata()?.len() > MAX_RECORD_BYTES {
            return Err(io::Error::other("local obligation record exceeds 2 MiB"));
        }
        drop(temp.persist(path)?);
        Ok(())
    }
}

pub(super) fn lock(path: &Path) -> io::Result<File> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing state directory"))?;
    fs::create_dir_all(parent)?;
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("lock"))?;
    file.try_lock().map_err(io::Error::from)?;
    Ok(file)
}
