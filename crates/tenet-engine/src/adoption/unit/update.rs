//! Update planning decides writes and merge outcomes before persistence begins.

use alloc::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tenet_contracts::with_name;

use super::{
    AdoptionError,
    File,
    Files,
    Kind,
    Outcome,
    Planned,
    Revision,
    Rule,
    Unit,
    Upstream,
    decode_text,
    find_obstruction,
    merge,
    read,
};

impl Unit {
    /// Decide how each file takes the release `next`, without writing anything. `edited_paths`
    /// holds the edited Markdown files, as [`Unit::ensure_updatable`] found them; every other
    /// locked file matches its hash. An unedited file is replaced, edited Markdown merges three
    /// ways against the locked commit, a new upstream file is added, and a file upstream removed
    /// stays in place and leaves the lock. Refuses a local file or symlink in the way of a new
    /// upstream file.
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "The steps run in one order and share local state; splitting them would add \
                  forwarding functions."
    )]
    pub(in crate::adoption) fn plan_update(
        &self,
        root: &Path,
        name: &str,
        edited_paths: &BTreeSet<PathBuf>,
        upstream: &Upstream,
        revision: &Revision,
        next: Files,
    ) -> Result<Planned, AdoptionError> {
        let kind = self.read_kind();
        let mut base: Option<Files> = None;
        let mut planned = Planned {
            files: BTreeMap::new(),
            outcomes: BTreeMap::new(),
            writes: Vec::new(),
        };
        for (path, mut file) in next {
            let rule = kind.derive_rule(&path);
            let installed = self.folder.join(&path);
            if rule == Rule::Named {
                let document = with_name(decode_text(&file.bytes, &installed)?, name)?;
                // Report the upstream text's error as a message, so it never points into the local
                // file.
                kind.validate(&installed, &document).map_err(|error| {
                    AdoptionError::UnreadableRelease {
                        reference: revision.reference.clone(),
                        reason: match kind {
                            Kind::Contracts => "needs a newer Tenet",
                            Kind::Skills => "has an invalid SKILL.md",
                        },
                        error: Box::new(error),
                    }
                })?;
                file.bytes = document.into_bytes();
            }
            let next_hash = kind.hash(&path, &file.bytes)?;

            let outcome = match self.files.get(&path) {
                Some(locked) if *locked == next_hash => Outcome::Unchanged,
                Some(_) if !edited_paths.contains(&path) => {
                    planned.writes.push((installed, file));
                    Outcome::Replaced
                },
                Some(_) => {
                    let local = read(root, &installed)?;
                    if base.is_none() {
                        base = Some(
                            upstream
                                .read_folder(&self.commit, kind, self.origin.name())?
                                .ok_or_else(|| self.report_missing_at_lock())?,
                        );
                    }
                    let base_file = base
                        .as_ref()
                        .and_then(|base_files| base_files.get(&path))
                        .ok_or_else(|| {
                            AdoptionError::MissingAtLock {
                                commit: self.commit.clone(),
                                name: path.display().to_string(),
                            }
                        })?;
                    let mut base_text = decode_text(&base_file.bytes, &installed)?.to_owned();
                    if rule == Rule::Named {
                        base_text = with_name(&base_text, name)?;
                    }
                    let merge = merge(
                        decode_text(&local, &installed)?,
                        &base_text,
                        decode_text(&file.bytes, &installed)?,
                        [
                            "local",
                            &format!("base {}", self.reference),
                            &format!("upstream {}", revision.reference),
                        ],
                    )?;
                    planned.writes.push((
                        installed,
                        File {
                            bytes: merge.text.into_bytes(),
                            executable: file.executable,
                        },
                    ));
                    if merge.conflicts == 0 {
                        Outcome::Merged
                    } else {
                        Outcome::Conflict {
                            regions: merge.conflicts,
                        }
                    }
                },
                None => {
                    match find_obstruction(root, &self.folder, &path)? {
                        None => {},
                        Some(found) if found == installed => {
                            return Err(AdoptionError::UnrecordedAtTarget {
                                path: installed,
                                reference: revision.reference.clone(),
                            });
                        },
                        Some(found) => {
                            return Err(AdoptionError::InTheWayOfUpdate {
                                found,
                                path: installed,
                                reference: revision.reference.clone(),
                            });
                        },
                    }
                    planned.writes.push((installed, file));
                    Outcome::Added
                },
            };
            let _: Option<Outcome> = planned.outcomes.insert(path.clone(), outcome);
            drop(planned.files.insert(path, next_hash));
        }
        for path in self.files.keys() {
            if !planned.files.contains_key(path) {
                let _: Option<Outcome> = planned.outcomes.insert(path.clone(), Outcome::Removed);
            }
        }
        Ok(planned)
    }
}
