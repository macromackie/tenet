use super::{CapturedFile, EvidencePacket, Role, Scope, ScopedFile, hash_content};
use crate::Mode;

impl EvidencePacket<'_> {
    /// Identify everything one contract's assessment covers: the contract document, mode, requested
    /// scope, and every selected and supporting file, with the lines of any excerpt. A
    /// full check also covers the scoped inventory, since a file that joins the scope is
    /// unverified. A diff check assumes the base complies, so a file the base gains outside the
    /// change leaves its input unchanged. Model settings are not part of it, so a record keeps
    /// applying to the code it describes.
    #[must_use]
    #[inline]
    pub fn derive_input_key(&self, contract: &tenet_contracts::Contract) -> String {
        let scope = Scope::new(self.plan, contract);
        let inventory = (self.plan.mode == Mode::Full).then_some(scope.inventory);
        // Changed lines follow from each file's contents, and keys made before Tenet recorded
        // them must still match.
        let mut files = self.list_scoped_files(contract);
        for scoped in &mut files {
            scoped.file.changed.clear();
        }
        let facts = serde_json::json!({
            "contract": contract.hash,
            "path": contract.path,
            "mode": self.plan.mode,
            "scope": scope.requested,
            "files": files,
            "inventory": inventory,
        });
        hash_content(&facts.to_string())
    }

    /// Identities of the selected and supporting files one contract's assessment covers.
    #[must_use]
    #[inline]
    pub fn list_scoped_files(&self, contract: &tenet_contracts::Contract) -> Vec<ScopedFile> {
        let scope = Scope::new(self.plan, contract);
        let selected = scope.selected.iter().map(|path| (*path, Role::Selected));
        let support = scope.support.iter().map(|path| (*path, Role::Support));
        selected
            .chain(support)
            .map(|(path, role)| {
                ScopedFile {
                    role,
                    file: match self.change(path) {
                        Ok(change) => {
                            CapturedFile {
                                path: path.clone(),
                                previous_path: change.previous_path.clone(),
                                before: change.before.as_deref().map(hash_content),
                                after: change.after.as_deref().map(hash_content),
                                error: None,
                                changed: match self.plan.mode {
                                    Mode::Diff => change.changed.clone(),
                                    Mode::Full => Vec::new(),
                                },
                                excerpt: (self.plan.excerpts.get(path))
                                    .map(|excerpt| excerpt.lines.clone())
                                    .unwrap_or_default(),
                            }
                        },
                        Err(error) => {
                            CapturedFile {
                                path: path.clone(),
                                previous_path: self.plan.previous_paths.get(path).cloned(),
                                before: None,
                                after: None,
                                error: Some(error.to_string()),
                                changed: Vec::new(),
                                excerpt: Vec::new(),
                            }
                        },
                    },
                }
            })
            .collect()
    }
}
