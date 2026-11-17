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
            // Retention pressure changes availability, never exact source identity.
            if self.changes.get(&scoped.file.path).is_some_and(|source| {
                source.has_complete_identity
                    && source
                        .change
                        .as_ref()
                        .err()
                        .is_some_and(|error| error.is_size_limit())
            }) {
                scoped.file.error = None;
            }
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

    /// Whether every selected and supporting source was read exactly and remained stable.
    #[must_use]
    #[inline]
    pub fn has_complete_identity(&self, contract: &tenet_contracts::Contract) -> bool {
        let scope = Scope::new(self.plan, contract);
        scope.selected.iter().chain(&scope.support).all(|path| {
            self.changes
                .get(*path)
                .is_some_and(|source| source.has_complete_identity)
        })
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
                let captured = self.changes.get(path);
                let change = captured.and_then(|source| source.change.as_ref().ok());
                ScopedFile {
                    role,
                    file: CapturedFile {
                        path: path.clone(),
                        previous_path: self.plan.previous_paths.get(path).cloned(),
                        before: captured.and_then(|source| source.before.clone()),
                        after: captured.and_then(|source| source.after.clone()),
                        error: self.change(path).err().map(|error| error.to_string()),
                        changed: match self.plan.mode {
                            Mode::Diff => {
                                change.map_or_else(Vec::new, |value| value.changed.clone())
                            },
                            Mode::Full => Vec::new(),
                        },
                        excerpt: self.plan.excerpts.get(path).cloned().unwrap_or_default(),
                    },
                }
            })
            .collect()
    }
}
