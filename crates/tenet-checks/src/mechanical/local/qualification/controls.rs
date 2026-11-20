use alloc::collections::BTreeSet;
use std::{
    fs,
    io::{self, Read as _},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tenet_engine::MAX_CAPTURE_BYTES;

use crate::mechanical::Outcome;

const MAX_CASES: usize = 64;
const MAX_MANIFEST_BYTES: usize = 0x0001_0000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Defect,
    Good,
    Refactor,
}

impl Kind {
    pub(super) const fn require_outcome(self) -> Outcome {
        match self {
            Self::Defect => Outcome::Fail,
            Self::Good | Self::Refactor => Outcome::Pass,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Case {
    pub finding_case: Option<String>,
    pub kind: Kind,
    pub name: String,
    pub patch: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Manifest {
    pub cases: Vec<Case>,
    pub subjects: Vec<PathBuf>,
    pub version: u8,
}

pub(super) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn read_bytes(path: &Path, max_bytes: usize) -> io::Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || path.canonicalize()? != path {
        return Err(io::Error::other("expected a regular file without symlinks"));
    }
    let limit = u64::try_from(max_bytes).unwrap_or(u64::MAX);
    if meta.len() > limit {
        return Err(io::Error::other("file exceeds the capture limit"));
    }
    let mut bytes = Vec::new();
    let _read = fs::File::open(path)?
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != meta.len() {
        return Err(io::Error::other("file changed during capture"));
    }
    Ok(bytes)
}

fn is_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

impl Manifest {
    pub(super) fn read(path: &Path) -> io::Result<(Self, String)> {
        let bytes = read_bytes(path, MAX_MANIFEST_BYTES).map_err(|error| {
            io::Error::new(error.kind(), format!("qualification manifest: {error:#}"))
        })?;
        let manifest: Self = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if manifest.version != 1 || manifest.cases.len() > MAX_CASES {
            return Err(io::Error::other(
                "expected version 1 and at most 64 qualification cases",
            ));
        }
        let mut subjects = BTreeSet::new();
        if manifest.subjects.is_empty()
            || !manifest
                .subjects
                .iter()
                .all(|subject| is_relative(subject) && subjects.insert(subject))
        {
            return Err(io::Error::other(
                "expected distinct project-relative subjects",
            ));
        }
        validate_cases(&manifest.cases)?;
        Ok((manifest, hash(&bytes)))
    }
}

pub(super) fn read_patches(folder: &Path, manifest: &Manifest) -> Vec<io::Result<Option<Vec<u8>>>> {
    let mut remaining = MAX_CAPTURE_BYTES;
    manifest
        .cases
        .iter()
        .map(|case| {
            case.patch
                .as_ref()
                .map(|path| {
                    let bytes = read_bytes(&folder.join(path), remaining).map_err(|error| {
                        io::Error::new(error.kind(), format!("control patch: {error:#}"))
                    })?;
                    remaining = remaining.saturating_sub(bytes.len());
                    Ok(bytes)
                })
                .transpose()
        })
        .collect()
}

fn validate_cases(cases: &[Case]) -> io::Result<()> {
    let mut names = BTreeSet::new();
    let mut kinds = BTreeSet::new();
    for case in cases {
        if case.name.trim().is_empty() || !names.insert(&case.name) {
            return Err(io::Error::other(
                "expected distinct nonempty qualification case names",
            ));
        }
        if case
            .patch
            .as_deref()
            .is_some_and(|patch| !is_relative(patch))
            || case.kind != Kind::Good && case.patch.is_none()
        {
            return Err(io::Error::other(
                "expected a relative patch for each defect and refactor",
            ));
        }
        if let Some(finding) = &case.finding_case
            && (finding.trim().is_empty() || case.kind != Kind::Defect)
        {
            return Err(io::Error::other(
                "finding_case must name a finding on a defect control",
            ));
        }
        let _new = kinds.insert(case.kind);
    }
    if kinds.len() != 3 {
        return Err(io::Error::other(
            "expected at least one good, defect and refactor control",
        ));
    }
    Ok(())
}
