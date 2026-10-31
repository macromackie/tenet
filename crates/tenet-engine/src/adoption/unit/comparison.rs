//! Comparisons describe upstream and local file changes without writing them.

use alloc::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tenet_contracts::hash_file;

use super::{AdoptionError, Change, FileComparison, Files, Rule, Unit, read};

impl Unit {
    /// The files other than the CONTRACT.md or SKILL.md that differ between the locked release,
    /// the newest release, and the local copy.
    pub(in crate::adoption) fn compare(
        &self,
        root: &Path,
        base: &Files,
        next: Option<&Files>,
    ) -> Result<Vec<FileComparison>, AdoptionError> {
        let kind = self.read_kind();
        let paths: BTreeSet<&PathBuf> = base
            .keys()
            .chain(next.into_iter().flat_map(Files::keys))
            .filter(|path| kind.derive_rule(path) != Rule::Named)
            .collect();
        let mut comparisons = Vec::new();
        for path in paths {
            let file = self.folder.join(path);
            let local = root
                .join(&file)
                .exists()
                .then(|| read(root, &file))
                .transpose()?;
            let versions = [
                base.get(path).map(|base_file| base_file.bytes.as_slice()),
                next.and_then(|next_files| next_files.get(path))
                    .map(|next_file| next_file.bytes.as_slice()),
                local.as_deref(),
            ];
            let hashes = versions.map(|version| version.map(hash_file));
            if hashes.iter().all(|hash| *hash == hashes[0]) {
                continue;
            }
            let [base_bytes, upstream, local_bytes] = versions
                .map(|version| version.map(|bytes| String::from_utf8_lossy(bytes).into_owned()));
            comparisons.push(FileComparison {
                path: path.clone(),
                base: base_bytes,
                upstream,
                local: local_bytes,
            });
        }
        Ok(comparisons)
    }

    /// Compare upstream's files at a newer release with the locked ones.
    pub(in crate::adoption) fn plan_changes(
        &self,
        next: &Files,
    ) -> Result<BTreeMap<PathBuf, Change>, AdoptionError> {
        let kind = self.read_kind();
        let mut changes = BTreeMap::new();
        for (path, file) in next {
            let change = match self.files.get(path) {
                None => Change::Added,
                Some(locked) if *locked == kind.hash(path, &file.bytes)? => Change::Current,
                Some(_) => Change::Changed,
            };
            let _: Option<Change> = changes.insert(path.clone(), change);
        }
        for path in self.files.keys().filter(|path| !next.contains_key(*path)) {
            let _: Option<Change> = changes.insert(path.clone(), Change::Removed);
        }
        Ok(changes)
    }
}
