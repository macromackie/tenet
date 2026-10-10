mod export;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{Change, Plan};

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
                let retained =
                    if let Some(limit) = error.downcast_ref::<crate::change::InputLimit>() {
                        anyhow::Error::new(*limit)
                    } else {
                        anyhow::anyhow!("{error:#}")
                    };
                retained.context(path.display().to_string())
            })
    }

    pub fn file_input(
        &self,
        contract: &tenet_contracts::Contract,
        path: &Path,
    ) -> Result<serde_json::Value> {
        let change = self.change(path)?;
        let context: Vec<_> = self
            .plan
            .context
            .iter()
            .filter(|support| support.path != change.path)
            .map(Change::current_input)
            .collect();
        let state = serde_json::json!({
            "contract": contract.body,
            "change": change,
            "files": [change.current_input()],
            "context": context,
        });
        crate::change::check_size("complete file input", serde_json::to_vec(&state)?.len())?;
        Ok(state)
    }
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
            for index in 0..40 {
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
