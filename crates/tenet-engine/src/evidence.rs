mod export;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    Change, Mode, Omitted, Plan,
    change::Unreadable,
    check::context::scope::Scope,
    review::{CapturedFile, Role, ScopedFile},
};

pub const MAX_CAPTURE_BYTES: usize = 64 * 1024 * 1024;

/// Source captured before assessment, shared by every request in one run.
pub struct EvidencePacket<'a> {
    pub(crate) plan: &'a Plan,
    changes: BTreeMap<PathBuf, Result<Change>>,
}

impl Plan {
    pub async fn capture(&self) -> Result<EvidencePacket<'_>> {
        self.capture_with_limits(MAX_CAPTURE_BYTES, MAX_CAPTURE_BYTES)
            .await
    }

    async fn capture_with_limits(
        &self,
        max_source_bytes: usize,
        max_capture_bytes: usize,
    ) -> Result<EvidencePacket<'_>> {
        let mut changes = BTreeMap::new();
        let mut source_bytes: usize = self.contracts.iter().map(|c| c.document.len()).sum();
        ensure!(
            source_bytes <= max_source_bytes,
            "contract documents exceed evidence limit of {max_source_bytes} bytes; input was not truncated"
        );
        let mut retained_bytes = 0;
        for contract in &self.contracts {
            count_retained(
                &(contract, &contract.document),
                &mut retained_bytes,
                max_capture_bytes,
            )?;
        }
        for change in &self.context {
            count_source(change, &mut source_bytes, max_source_bytes)?;
            count_retained(
                &(&change.path, change, &change.after),
                &mut retained_bytes,
                max_capture_bytes,
            )?;
            changes.insert(change.path.clone(), Ok(change.clone()));
        }
        for path in &self.files {
            tokio::task::yield_now().await;
            if let std::collections::btree_map::Entry::Vacant(entry) = changes.entry(path.clone()) {
                let change = self.read_change(path);
                match &change {
                    Ok(change) => {
                        count_source(change, &mut source_bytes, max_source_bytes)?;
                        count_retained(
                            &(path, change, &change.after),
                            &mut retained_bytes,
                            max_capture_bytes,
                        )?;
                    }
                    Err(error) => count_retained(
                        &(path, error.to_string()),
                        &mut retained_bytes,
                        max_capture_bytes,
                    )?,
                }
                entry.insert(change);
            }
        }
        Ok(EvidencePacket {
            plan: self,
            changes,
        })
    }
}

fn count_retained(value: &impl Serialize, bytes: &mut usize, limit: usize) -> Result<()> {
    *bytes = bytes.saturating_add(serde_json::to_vec(value)?.len());
    ensure!(
        *bytes <= limit,
        "captured inputs exceed {limit} encoded bytes; select fewer paths or contracts; input was not truncated"
    );
    Ok(())
}

fn count_source(change: &Change, bytes: &mut usize, max_bytes: usize) -> Result<()> {
    *bytes = bytes.saturating_add(
        change.before.as_ref().map_or(0, String::len)
            + change.after.as_ref().map_or(0, String::len),
    );
    ensure!(
        *bytes <= max_bytes,
        "captured source exceeds evidence limit of {max_bytes} bytes; input was not truncated"
    );
    Ok(())
}

impl EvidencePacket<'_> {
    pub(crate) fn change(&self, path: &Path) -> Result<&Change> {
        self.changes
            .get(path)
            .with_context(|| format!("source was not captured: {}", path.display()))?
            .as_ref()
            .map_err(|error| {
                let retained = if let Some(unreadable) = error.downcast_ref::<Unreadable>() {
                    anyhow::Error::new(*unreadable)
                } else {
                    anyhow::anyhow!("{error:#}")
                };
                retained.context(path.display().to_string())
            })
    }

    /// The model input for one file's check; an empty file has none. When the complete input
    /// exceeds the limit, prior and then resulting contents are left out, but only when the patch
    /// remains to show the change.
    pub fn file_input(
        &self,
        contract: &tenet_contracts::Contract,
        path: &Path,
    ) -> Result<(serde_json::Value, Vec<Omitted>)> {
        let change = self.change(path)?;
        if change.is_empty() {
            return Err(Unreadable::Empty.into());
        }
        let context: Vec<_> = self
            .plan
            .context
            .iter()
            .filter(|support| support.path != change.path)
            .map(Change::current_input)
            .collect();
        let mut state = serde_json::json!({
            "contract": contract.body,
            "change": change,
            "files": [change.current_input()],
            "context": context,
        });
        let mut omitted = Vec::new();
        let patched = !change.patch.is_empty();
        for (part, present) in [
            (Omitted::Before, change.before.is_some() && patched),
            (Omitted::Contents, change.after.is_some() && patched),
        ] {
            if !present || serde_json::to_vec(&state)?.len() <= crate::MAX_FILE_BYTES {
                continue;
            }
            let slot = match part {
                Omitted::Before => &mut state["change"]["before"],
                Omitted::Contents => &mut state["files"][0]["contents"],
            };
            *slot = crate::change::omitted("too large; judge from the patch");
            omitted.push(part);
        }
        crate::change::check_size(
            "file input with only its patch",
            serde_json::to_vec(&state)?.len(),
        )?;
        Ok((state, omitted))
    }

    /// Identities of the selected and supporting files one contract's assessment covers.
    pub fn scoped_files(&self, contract: &tenet_contracts::Contract) -> Vec<ScopedFile> {
        let scope = Scope::new(self.plan, contract);
        let selected = scope.selected.iter().map(|path| (*path, Role::Selected));
        let support = scope.support.iter().map(|path| (*path, Role::Support));
        selected
            .chain(support)
            .map(|(path, role)| ScopedFile {
                role,
                file: match self.change(path) {
                    Ok(change) => CapturedFile {
                        path: path.clone(),
                        previous_path: change.previous_path.clone(),
                        before: change.before.as_deref().map(content_hash),
                        after: change.after.as_deref().map(content_hash),
                        error: None,
                        changed: match self.plan.mode {
                            Mode::Diff => change.changed.clone(),
                            Mode::Full => Vec::new(),
                        },
                    },
                    Err(error) => CapturedFile {
                        path: path.clone(),
                        previous_path: self.plan.previous_paths.get(path).cloned(),
                        before: None,
                        after: None,
                        error: Some(format!("{error:#}")),
                        changed: Vec::new(),
                    },
                },
            })
            .collect()
    }

    /// Identify everything one contract's assessment covers: the contract document, mode, requested
    /// scope, and every selected and supporting file. A full check also covers the scoped inventory,
    /// since a file that joins the scope is unverified. A diff check assumes the base complies, so a
    /// file the base gains outside the change leaves its input unchanged. Model settings are not
    /// part of it, so a record keeps applying to the code it describes.
    pub fn input_key(&self, contract: &tenet_contracts::Contract) -> String {
        let scope = Scope::new(self.plan, contract);
        let inventory = (self.plan.mode == Mode::Full).then_some(scope.inventory);
        // Changed lines follow from each file's contents, and keys made before Tenet recorded
        // them must still match.
        let mut files = self.scoped_files(contract);
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
        content_hash(&facts.to_string())
    }
}

/// How a file whose model input cannot be built is reported: skipped when it has no text,
/// incomplete when its patch alone exceeds the input limit, and otherwise an error.
pub fn input_status(error: &anyhow::Error) -> (crate::Status, String) {
    if let Some(unreadable) = error.downcast_ref::<Unreadable>() {
        (crate::Status::Skipped, unreadable.to_string())
    } else if error.is::<crate::change::InputLimit>() {
        (crate::Status::Incomplete, format!("{error:#}"))
    } else {
        (crate::Status::Error, format!("{error:#}"))
    }
}

pub(crate) fn content_hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use anyhow::Result;

    use crate::{Change, Selection, plan};

    fn fixture() -> Result<tempfile::TempDir> {
        let root = tempfile::tempdir()?;
        fs::create_dir_all(root.path().join(".contracts/001-errors"))?;
        fs::write(
            root.path().join(".contracts/001-errors/CONTRACT.md"),
            "---\nname: errors\nmessage: Preserve failures.\n---\n## Rules\nPropagate failures.\n",
        )?;
        Ok(root)
    }

    #[tokio::test]
    async fn empty_and_unreadable_entries_still_exhaust_the_capture_budget() -> Result<()> {
        for contents in ["", "\0"] {
            let root = fixture()?;
            for index in 0..80 {
                fs::write(root.path().join(format!("source-{index}.rs")), contents)?;
            }
            let plan = plan(root.path(), &Selection::default())?;
            let error = plan.capture_with_limits(1024, 2048).await.err();
            assert!(error.is_some_and(|error| error.to_string().contains("captured inputs")));
        }
        Ok(())
    }

    #[tokio::test]
    async fn capture_charges_patches_and_deduplicates_selected_context() -> Result<()> {
        let root = fixture()?;
        fs::write(root.path().join("file.rs"), "x".repeat(1000))?;
        let mut plan =
            plan(root.path(), &Selection::default())?.with_context(&["file.rs".into()])?;
        let evidence = plan.capture_with_limits(1500, 4096).await?;
        assert_eq!(evidence.changes.len(), 1);
        drop(evidence);

        plan.context[0] = Change::new(
            "file.rs".into(),
            Some("a\n".repeat(500)),
            Some("b\n".repeat(500)),
        );
        let error = plan.capture_with_limits(3000, 4096).await.err();
        assert!(error.is_some_and(|error| error.to_string().contains("captured inputs")));
        Ok(())
    }
}
