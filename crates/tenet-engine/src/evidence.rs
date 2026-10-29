mod export;

use alloc::{
    collections::{BTreeMap, btree_map::Entry},
    sync::Arc,
};
use core::mem;
use std::{
    io,
    path::{Path, PathBuf},
};

use serde::Serialize;
use tokio::task;

use crate::{
    Change,
    Mode,
    Omitted,
    Plan,
    change,
    change::{InputLimit, SourceError, Unreadable},
    check::context::scope::Scope,
    plan::ExcerptError,
    report::Causes,
    review::{CapturedFile, Role, ScopedFile},
};

/// The most bytes a check captures from the files it reads.
pub const MAX_CAPTURE_BYTES: usize = 64 * 1024 * 1024;

/// Why evidence cannot be captured or exported within its limits.
#[derive(Debug, thiserror::Error)]
pub enum EvidenceError {
    #[error(
        "captured inputs exceed {0:?} encoded bytes; select fewer paths or contracts; input was \
         not truncated"
    )]
    CaptureTooLarge(usize),
    #[error("contract documents exceed evidence limit of {0:?} bytes; input was not truncated")]
    ContractsTooLarge(usize),
    #[error(transparent)]
    Encode(#[from] serde_json::Error),
    #[error("evidence limit must be between 1 and 67108864 bytes")]
    InvalidLimit,
    #[error("captured source exceeds evidence limit of {0:?} bytes; input was not truncated")]
    SourceTooLarge(usize),
    #[error(transparent)]
    Write(#[from] io::Error),
}

/// Why a selected or supporting file's change was not captured. Every check of the file reports
/// it.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error(transparent)]
    Excerpt(#[from] ExcerptError),
    #[error(transparent)]
    Source(#[from] SourceError),
}

/// Why a file's captured change cannot be judged, naming the file.
#[derive(Clone, Debug, thiserror::Error)]
pub enum ChangeError {
    #[error("source was not captured: {:?}", .0.display())]
    NotCaptured(PathBuf),
    #[error("{:?}: {:#}", .path.display(), Causes(&**.error))]
    Uncaptured {
        path: PathBuf,
        error: Arc<CaptureError>,
    },
    /// The file has no text a model can judge.
    #[error("{:?}: {reason:#}", .path.display())]
    Unreadable { path: PathBuf, reason: Unreadable },
}

/// Why a file's model input cannot be built.
#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error(transparent)]
    Change(#[from] ChangeError),
    #[error(transparent)]
    Encode(#[from] serde_json::Error),
    #[error(transparent)]
    Excerpt(#[from] ExcerptError),
    #[error(transparent)]
    Limit(#[from] InputLimit),
    #[error(transparent)]
    Unreadable(#[from] Unreadable),
}

/// Source captured before assessment, shared by every request in one run.
#[derive(Debug)]
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "The sibling modules fill these fields, and the type stays private to its parent \
              module."
)]
pub struct EvidencePacket<'plan> {
    changes: BTreeMap<PathBuf, Result<Change, Arc<CaptureError>>>,
    pub(crate) plan: &'plan Plan,
}

impl Plan {
    /// Read every planned file once, so each check sees the same contents.
    #[inline]
    pub async fn capture(&self) -> Result<EvidencePacket<'_>, EvidenceError> {
        self.capture_with_limits(MAX_CAPTURE_BYTES, MAX_CAPTURE_BYTES)
            .await
    }

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
    #[expect(
        clippy::let_underscore_must_use,
        reason = "The inserted slot is not used after the insert."
    )]
    async fn capture_with_limits(
        &self,
        max_source_bytes: usize,
        max_capture_bytes: usize,
    ) -> Result<EvidencePacket<'_>, EvidenceError> {
        let mut changes = BTreeMap::new();
        let mut source_bytes: usize = self
            .contracts
            .iter()
            .map(|contract| contract.document.len())
            .sum();
        if source_bytes > max_source_bytes {
            return Err(EvidenceError::ContractsTooLarge(max_source_bytes));
        }
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
            // A file that cannot supply its named lines is captured as an error, as an unreadable
            // selected file is.
            let captured = match self.excerpts.get(&change.path) {
                Some(excerpt) => {
                    (excerpt.contents())
                        .map(|_| change.clone())
                        .map_err(|error| Arc::new(error.into()))
                },
                None => Ok(change.clone()),
            };
            drop(changes.insert(change.path.clone(), captured));
        }
        for path in &self.files {
            task::yield_now().await;
            if let Entry::Vacant(entry) = changes.entry(path.clone()) {
                let change =
                    (self.read_change(path)).map_err(|error| Arc::new(CaptureError::Source(error)));
                match &change {
                    Ok(captured) => {
                        count_source(captured, &mut source_bytes, max_source_bytes)?;
                        count_retained(
                            &(path, captured, &captured.after),
                            &mut retained_bytes,
                            max_capture_bytes,
                        )?;
                    },
                    Err(error) => {
                        count_retained(
                            &(path, error.to_string()),
                            &mut retained_bytes,
                            max_capture_bytes,
                        )?;
                    },
                }
                let _: &mut Result<_, _> = entry.insert(change);
            }
        }
        Ok(EvidencePacket {
            changes,
            plan: self,
        })
    }
}

fn count_retained(
    value: &impl Serialize,
    bytes: &mut usize,
    limit: usize,
) -> Result<(), EvidenceError> {
    *bytes = bytes.saturating_add(serde_json::to_vec(value)?.len());
    if *bytes > limit {
        return Err(EvidenceError::CaptureTooLarge(limit));
    }
    Ok(())
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "The byte counts are bounded by the capture size caps."
)]
fn count_source(change: &Change, bytes: &mut usize, max_bytes: usize) -> Result<(), EvidenceError> {
    *bytes = bytes.saturating_add(
        change.before.as_ref().map_or(0, String::len)
            + change.after.as_ref().map_or(0, String::len),
    );
    if *bytes > max_bytes {
        return Err(EvidenceError::SourceTooLarge(max_bytes));
    }
    Ok(())
}

impl EvidencePacket<'_> {
    /// The model input for one file's check; an empty file has none. When the complete input
    /// exceeds the limit, prior and then resulting contents are left out, but only when the patch
    /// remains to show the change. Lines of the file named with `--context` stand in for its
    /// resulting contents where they fit.
    #[expect(
        clippy::indexing_slicing,
        reason = "This function builds `state` with a `files` array that holds one file and a \
                  `change` object, so serde_json's index finds both."
    )]
    #[inline]
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    pub fn build_file_input(
        &self,
        contract: &tenet_contracts::Contract,
        path: &Path,
    ) -> Result<(serde_json::Value, Vec<Omitted>), InputError> {
        let change = self.change(path)?;
        if change.is_empty() {
            return Err(Unreadable::Empty.into());
        }
        let context: Vec<_> = self
            .plan
            .context
            .iter()
            .filter(|support| support.path != change.path)
            .map(|support| self.plan.build_context_input(support))
            .collect::<Result<_, _>>()?;
        let mut state = serde_json::json!({
            "contract": contract.body,
            "change": change,
            "files": [change.describe_current_input()],
            "context": context,
        });
        let mut omitted = Vec::new();
        let patched = !change.patch.is_empty();
        let excerpt = self.plan.excerpts.get(&change.path);
        for (part, present) in [
            (Omitted::Before, change.before.is_some() && patched),
            (Omitted::Contents, change.after.is_some() && patched),
        ] {
            if !present || serde_json::to_vec(&state)?.len() <= crate::MAX_FILE_BYTES {
                continue;
            }
            let too_large = change::describe_omitted("too large; judge from the patch");
            match part {
                Omitted::Before => state["change"]["before"] = too_large,
                Omitted::Contents => {
                    state["files"][0]["contents"] = too_large;
                    // The named lines stand in for the contents only where they fit.
                    if let Some(named) = excerpt {
                        let placeholder =
                            mem::replace(&mut state["files"][0]["contents"], named.contents()?);
                        if serde_json::to_vec(&state)?.len() > crate::MAX_FILE_BYTES {
                            state["files"][0]["contents"] = placeholder;
                        }
                    }
                },
            }
            omitted.push(part);
        }
        change::check_size(
            "file input with only its patch",
            serde_json::to_vec(&state)?.len(),
        )?;
        Ok((state, omitted))
    }

    pub(crate) fn change(&self, path: &Path) -> Result<&Change, ChangeError> {
        match self.changes.get(path) {
            Some(Ok(change)) => Ok(change),
            Some(Err(error)) => {
                Err(match &**error {
                    CaptureError::Source(SourceError::Unreadable(reason)) => {
                        ChangeError::Unreadable {
                            path: path.to_owned(),
                            reason: *reason,
                        }
                    },
                    CaptureError::Source(_) | CaptureError::Excerpt(_) => {
                        ChangeError::Uncaptured {
                            path: path.to_owned(),
                            error: Arc::clone(error),
                        }
                    },
                })
            },
            None => Err(ChangeError::NotCaptured(path.to_owned())),
        }
    }

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

/// How a file whose model input cannot be built is reported: skipped when it has no text,
/// incomplete when its patch alone exceeds the input limit, and otherwise an error.
#[must_use]
#[inline]
pub fn classify_input_status(error: &InputError) -> (crate::Status, String) {
    match error {
        InputError::Unreadable(reason)
        | InputError::Change(ChangeError::Unreadable { reason, .. }) => {
            (crate::Status::Skipped, reason.to_string())
        },
        InputError::Limit(_) => (crate::Status::Incomplete, error.to_string()),
        InputError::Change(ChangeError::Uncaptured { .. } | ChangeError::NotCaptured(_))
        | InputError::Excerpt(_)
        | InputError::Encode(_) => (crate::Status::Error, Causes(error).to_string()),
    }
}

pub(crate) fn hash_content(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    reason = "Unit tests stay beside the private functions they test."
)]
mod tests {
    use std::fs;

    use anyhow::Result;

    use crate::{Change, Selection, plan};

    fn create_fixture() -> Result<tempfile::TempDir> {
        let root = tempfile::tempdir()?;
        fs::create_dir_all(root.path().join(".contracts/001-errors"))?;
        fs::write(
            root.path().join(".contracts/001-errors/CONTRACT.md"),
            "---\nname: errors\nmessage: Preserve failures.\n---\n## Rules\nPropagate failures.\n",
        )?;
        Ok(root)
    }

    #[tokio::test]
    #[expect(
        clippy::excessive_nesting,
        reason = "The test's fixture closures and loops nest inside the scenario they build."
    )]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "A test reports its failure by panicking."
    )]
    async fn empty_and_unreadable_entries_still_exhaust_the_capture_budget() -> Result<()> {
        for contents in ["", "\0"] {
            let root = create_fixture()?;
            for index in 0..80_u32 {
                fs::write(root.path().join(format!("source-{index}.rs")), contents)?;
            }
            let plan = plan(root.path(), &Selection::default())?;
            let error = plan.capture_with_limits(1024, 2048).await.err();
            assert!(error.is_some_and(|failure| failure.to_string().contains("captured inputs")));
        }
        Ok(())
    }

    #[tokio::test]
    #[expect(
        clippy::indexing_slicing,
        reason = "A missing element panics, and the test harness reports that panic as this \
                  test's failure."
    )]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "A test reports its failure by panicking."
    )]
    async fn capture_charges_patches_and_deduplicates_selected_context() -> Result<()> {
        let root = create_fixture()?;
        fs::write(root.path().join("file.rs"), "x".repeat(1000))?;
        let mut plan =
            plan(root.path(), &Selection::default())?.with_context(&["file.rs".parse()?])?;
        let evidence = plan.capture_with_limits(1500, 4096).await?;
        assert_eq!(evidence.changes.len(), 1);
        drop(evidence);

        plan.context[0] = Change::new(
            "file.rs".into(),
            Some("a\n".repeat(500)),
            Some("b\n".repeat(500)),
        );
        let error = plan.capture_with_limits(3000, 4096).await.err();
        assert!(error.is_some_and(|failure| failure.to_string().contains("captured inputs")));
        Ok(())
    }
}
