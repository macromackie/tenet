//! Local state checks refuse missing or edited read-only files before an update.

use alloc::collections::{BTreeMap, BTreeSet};
use std::{
    fs,
    path::{Path, PathBuf},
};

use tenet_contracts::name_of;

use super::{AdoptionError, LocalState, Rule, Unit, read};

impl Unit {
    /// Refuse a missing locked file or an edited read-only one, as `validate` and `update` do, and
    /// return the adapted or edited Markdown files, which merge, with their states.
    pub(in crate::adoption) fn check_files(
        &self,
        root: &Path,
    ) -> Result<BTreeMap<PathBuf, LocalState>, AdoptionError> {
        let kind = self.read_kind();
        let mut edited_paths = BTreeMap::new();
        for (path, state) in self.read_local_states(root)? {
            let file = self.folder.join(&path);
            match (state, kind.derive_rule(&path)) {
                (LocalState::Missing, _) => {
                    return Err(AdoptionError::MissingFile {
                        file,
                        kind,
                        key: self.key.clone(),
                    });
                },
                (LocalState::Adapted | LocalState::Edited, Rule::ReadOnly) => {
                    return Err(AdoptionError::EditedReadOnly {
                        file,
                        kind,
                        key: self.key.clone(),
                    });
                },
                (LocalState::Adapted | LocalState::Edited, Rule::Named | Rule::Merged) => {
                    let _: Option<LocalState> = edited_paths.insert(path, state);
                },
                (LocalState::Unchanged, _) => {},
            }
        }
        Ok(edited_paths)
    }

    /// The local name, after validating the CONTRACT.md or SKILL.md as an update reads it.
    pub(in crate::adoption) fn derive_local_name(
        &self,
        root: &Path,
    ) -> Result<String, AdoptionError> {
        let document = self.name_document();
        let text = fs::read_to_string(root.join(&document)).map_err(|source| {
            AdoptionError::Read {
                path: document.clone(),
                source,
            }
        })?;
        self.read_kind().validate(&document, &text)?;
        Ok(name_of(&text)?.to_owned())
    }

    /// Refuse an update, before anything is fetched or written, as [`Unit::check_files`] does,
    /// and return the adapted or edited Markdown files.
    pub(in crate::adoption) fn ensure_updatable(
        &self,
        root: &Path,
    ) -> Result<BTreeSet<PathBuf>, AdoptionError> {
        let document = self.name_document();
        if !root.join(&document).is_file() {
            return Err(AdoptionError::DocumentMissing {
                document,
                kind: self.read_kind(),
                key: self.key.clone(),
            });
        }
        Ok(self.check_files(root)?.into_keys().collect())
    }

    /// Each locked file's local state.
    pub(in crate::adoption) fn read_local_states(
        &self,
        root: &Path,
    ) -> Result<BTreeMap<PathBuf, LocalState>, AdoptionError> {
        let kind = self.read_kind();
        self.files
            .iter()
            .map(|(path, locked)| {
                let file = self.folder.join(path);
                if !root.join(&file).exists() {
                    return Ok((path.clone(), LocalState::Missing));
                }
                let state = kind
                    .derive_state(path, &read(root, &file)?, locked)
                    .map_err(|source| {
                        AdoptionError::HashFile {
                            path: file.clone(),
                            source,
                        }
                    })?;
                Ok((path.clone(), state))
            })
            .collect()
    }
}
