use alloc::{
    collections::{BTreeMap, btree_map::Entry},
    sync::Arc,
};

use serde::Serialize;
use tokio::task;

use super::{CaptureError, EvidenceError, EvidencePacket};
use crate::{Change, Plan, change::MAX_CAPTURE_BYTES};

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
    pub(super) async fn capture_with_limits(
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
