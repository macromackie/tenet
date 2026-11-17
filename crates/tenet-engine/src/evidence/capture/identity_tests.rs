use std::fs::{self, File, FileTimes};
#[cfg(unix)]
use std::os::unix::fs::symlink;

use anyhow::Result;

use super::fixture::create_fixture;
use crate::{Review, Selection, plan};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A test reports its failure by panicking."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "The fixture installs one contract whose absence must fail the test."
)]
async fn failed_base_still_hashes_current_source_and_cannot_assert_source_truth() -> Result<()> {
    let root = create_fixture(&[])?;
    fs::write(root.path().join("file.rs"), "before")?;
    let mut selected = plan(root.path(), &Selection::default())?;
    selected.base = Some("missing-base".into());
    let first = selected.capture_with_limits(1024, 8192).await?;
    let key = first.derive_input_key(&selected.contracts[0]);
    let files = first.list_scoped_files(&selected.contracts[0]);
    assert!(files.iter().all(|scoped| scoped.file.after.is_some()));
    assert!(!first.has_complete_identity(&selected.contracts[0]));
    fs::write(root.path().join("file.rs"), "after!")?;
    let after = selected.capture_with_limits(1024, 8192).await?;
    assert_ne!(key, after.derive_input_key(&selected.contracts[0]));
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A test reports its failure by panicking."
)]
#[expect(
    clippy::too_many_lines,
    reason = "One scenario follows both substantive manual conclusions through capture, commit, \
              record and a source edit."
)]
async fn deferred_sources_commit_and_same_size_edits_stale_manual_approvals_and_failures()
-> Result<()> {
    use crate::{
        ContractStatus,
        review::{self, CheckRequest, RecordRequest},
    };
    for conclusion in [ContractStatus::Verified, ContractStatus::Failed] {
        let text = "line\n".repeat(512);
        let root = create_fixture(&[("big.rs", &text), ("small.rs", "fn small() {}\n")])?;
        let selection = Selection::default();
        let selected = plan(root.path(), &selection)?;
        let evidence = selected.capture_with_limits(1024, 8192).await?;
        let state = root.path().join(review::DIRECTORY);
        let mut saved = Review::open(&state)?;
        let checked = crate::apply_review(&evidence, &saved, &|_| Ok::<_, anyhow::Error>(()))?;
        let revision = saved.commit(
            &evidence,
            &CheckRequest {
                context: &[],
                contract_batch_size: 1,
                min_confidence: None,
                model: None,
                selection: &selection,
            },
            &checked,
        )?;
        assert_eq!(revision, Some(1));
        let recorded = review::record(
            root.path(),
            &state,
            &RecordRequest {
                conclusion,
                contract: "errors".into(),
                evidence: vec!["small.rs:1".parse()?],
                message: "Inspected all source".into(),
                paths: Vec::new(),
            },
        )
        .await?;
        let reopened = Review::open_existing(&state)?;
        let current = review::resolve_current(root.path(), &reopened, None, None).await?;
        let row = current
            .contracts
            .first()
            .ok_or_else(|| anyhow::anyhow!("missing current contract"))?;
        assert_eq!(
            row.state,
            crate::ContractState::Settled {
                conclusion,
                record: Some(recorded.record.id)
            }
        );
        let source_path = root.path().join("big.rs");
        let source_modified = fs::metadata(&source_path)?.modified()?;
        let mut changed = text.into_bytes();
        *changed
            .last_mut()
            .ok_or_else(|| anyhow::anyhow!("empty source"))? = b'!';
        fs::write(&source_path, changed)?;
        File::open(&source_path)?.set_times(FileTimes::new().set_modified(source_modified))?;
        assert_eq!(fs::metadata(source_path)?.modified()?, source_modified);
        let changed_current = review::resolve_current(root.path(), &reopened, None, None).await?;
        let changed_row = changed_current
            .contracts
            .first()
            .ok_or_else(|| anyhow::anyhow!("missing changed contract"))?;
        assert_eq!(
            changed_row.state,
            crate::ContractState::Pending {
                reason: crate::Pending::InputsChanged
            }
        );
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A test reports its failure by panicking."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "The fixture installs one contract whose absence must fail the test."
)]
async fn a_same_bytes_symlink_cannot_keep_a_regular_source_identity() -> Result<()> {
    let root = create_fixture(&[("subject.rs", "target.rs"), ("target.rs", "fn target() {}")])?;
    let selected = plan(root.path(), &Selection::default())?;
    let first = selected.capture().await?;
    let key = first.derive_input_key(&selected.contracts[0]);
    fs::remove_file(root.path().join("subject.rs"))?;
    symlink("target.rs", root.path().join("subject.rs"))?;
    let after = selected.capture().await?;
    assert!(after.has_complete_identity(&selected.contracts[0]));
    assert_ne!(key, after.derive_input_key(&selected.contracts[0]));
    Ok(())
}
