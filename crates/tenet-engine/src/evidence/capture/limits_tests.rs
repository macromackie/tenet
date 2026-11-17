use std::{fs, path::PathBuf};

use anyhow::Result;

use super::{budget::CaptureBudget, fixture::create_fixture};
use crate::{
    Change,
    Review,
    Selection,
    Status,
    classify_input_status,
    estimate,
    evidence::source::CapturedSource,
    plan,
};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A test reports its failure by panicking."
)]
async fn empty_and_unreadable_entries_still_exhaust_the_capture_budget() -> Result<()> {
    for contents in ["", "\0"] {
        let root = create_fixture(&[])?;
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
    reason = "A missing element panics, and the test harness reports that panic as this test's \
              failure."
)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A test reports its failure by panicking."
)]
async fn capture_charges_patches_and_deduplicates_selected_context() -> Result<()> {
    let root = create_fixture(&[])?;
    fs::write(root.path().join("file.rs"), "x".repeat(1000))?;
    let plan = plan(root.path(), &Selection::default())?.with_context(&["file.rs".parse()?])?;
    let evidence = plan.capture_with_limits(1500, 4096).await?;
    assert_eq!(evidence.changes.len(), 1);
    drop(evidence);

    let mut budget = CaptureBudget::new(0, 0, 3000, 4096);
    let path = PathBuf::from("file.rs");
    budget.insert(
        path.clone(),
        CapturedSource {
            after: Some("after".into()),
            before: Some("before".into()),
            change: Ok(Change::new(
                path.clone(),
                Some("a\n".repeat(500)),
                Some("b\n".repeat(500)),
            )),
            has_complete_identity: true,
        },
        None,
    )?;
    let bounded = budget.into_packet(&plan);
    let error = bounded
        .build_file_input(&plan.contracts[0], &path)
        .err()
        .ok_or_else(|| anyhow::anyhow!("encoded patch limit was ignored"))?;
    assert_eq!(classify_input_status(&error).0, Status::Incomplete);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A test reports its failure by panicking."
)]
async fn aggregate_limits_preserve_late_checks_and_exact_keys() -> Result<()> {
    let root = create_fixture(&[])?;
    for name in ["a.rs", "b.rs", "c.rs", "d.rs"] {
        fs::write(root.path().join(name), "x".repeat(1200))?;
    }
    fs::write(root.path().join("zz-small.rs"), "fn small() {}\n")?;
    fs::create_dir_all(root.path().join(".contracts/002-small"))?;
    fs::write(
        root.path().join(".contracts/002-small/CONTRACT.md"),
        "---\nname: small\nmessage: Small control.\npaths: [zz-small.rs]\n---\n## Rules\nPreserve \
         failures.\n",
    )?;
    let plan = plan(root.path(), &Selection::default())?;
    let full = plan.capture().await?;
    for (raw, encoded) in [(4000, 12000), (10000, 4000)] {
        let bounded = plan.capture_with_limits(raw, encoded).await?;
        assert_eq!(bounded.changes.len(), plan.files.len());
        assert!(bounded.changes.values().any(|source| {
            source
                .change
                .as_ref()
                .err()
                .is_some_and(|error| error.is_size_limit())
        }));
        for contract in &plan.contracts {
            assert!(bounded.has_complete_identity(contract));
            assert_eq!(
                bounded.derive_input_key(contract),
                full.derive_input_key(contract)
            );
        }
        let prediction = estimate(&bounded, &Review::new_ephemeral(), 1, 1).await?;
        assert_eq!(prediction.contract_requests.get("small"), Some(&2));
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A test reports its failure by panicking."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "The fixture installs one contract whose absence must fail the test."
)]
async fn selected_and_supporting_overflow_remains_incomplete() -> Result<()> {
    let text = "line\n".repeat(512);
    let root = create_fixture(&[("big.rs", &text), ("small.rs", "fn small() {}\n")])?;
    let selected = plan(root.path(), &Selection::default())?;
    let first = selected.capture_with_limits(1024, 8192).await?;
    let error = first
        .build_file_input(&selected.contracts[0], "big.rs".as_ref())
        .err()
        .ok_or_else(|| anyhow::anyhow!("oversized source was admitted"))?;
    assert_eq!(classify_input_status(&error).0, Status::Incomplete);
    assert!(first.has_complete_identity(&selected.contracts[0]));
    for request in ["big.rs", "big.rs:1-2"] {
        let supporting = plan(
            root.path(),
            &Selection {
                paths: vec!["small.rs".into()],
                ..Selection::default()
            },
        )?
        .with_context(&[request.parse()?])?;
        let evidence = supporting.capture_with_limits(1024, 8192).await?;
        assert!(
            evidence
                .admit_context()
                .err()
                .is_some_and(|admission| admission.is_size_limit())
        );
        let context_error = evidence
            .build_file_input(&supporting.contracts[0], "small.rs".as_ref())
            .err()
            .ok_or_else(|| anyhow::anyhow!("oversized context was admitted"))?;
        assert_eq!(classify_input_status(&context_error).0, Status::Incomplete);
        assert!(evidence.has_complete_identity(&supporting.contracts[0]));
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A test reports its failure by panicking."
)]
async fn payload_pressure_cannot_turn_a_malformed_excerpt_into_a_size_exclusion() -> Result<()> {
    let root = create_fixture(&[])?;
    fs::write(root.path().join("helper.rs"), "one\ntwo\n")?;
    for name in ["a.rs", "b.rs", "c.rs"] {
        fs::write(root.path().join(name), "x".repeat(600))?;
    }
    let selected =
        plan(root.path(), &Selection::default())?.with_context(&["helper.rs:1-10".parse()?])?;
    let evidence = selected.capture_with_limits(1024, 8192).await?;
    let error = evidence
        .admit_context()
        .err()
        .ok_or_else(|| anyhow::anyhow!("malformed context was admitted"))?;
    assert!(!error.is_size_limit());
    assert!(matches!(error, crate::ContextError::Excerpt(_)));
    Ok(())
}
