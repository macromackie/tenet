use std::{fs, path::Path};

use anyhow::Result;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use tenet_engine::{
    MAX_CAPTURE_BYTES,
    mechanical::{Freshness, Outcome, verify},
};

const CONTRACT: &str = concat!(
    "---\nname: parsers\nmessage: Preserve input presence.\n",
    "obligations: [first-present, no-fallback]\npaths: ['src/**/*.py']\n",
    "---\n## Rules\nOnly read the winning source and earlier sources.\n",
);
const DOCUMENT: &str = ".contracts/001-parsers/CONTRACT.md";
const PLAN: &str = "checker.toml";
const SOURCE: &str = "src/options.py";
const SUPPORT: &str = "adapter.py";
const RECEIPTS: &str = "receipt.json";

fn hash_text(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn create_fixture() -> Result<(tempfile::TempDir, Value)> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path();
    fs::create_dir_all(root.join(".contracts/001-parsers"))?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join(DOCUMENT), CONTRACT)?;
    fs::write(root.join(PLAN), "strict = true\n")?;
    fs::write(root.join(SOURCE), "VALUE = 1\n")?;
    fs::write(root.join(SUPPORT), "def adapt(value): return value\n")?;
    let receipt = json!({
        "checker": "property-runner@1",
        "contract": "parsers",
        "contract_sha256": hash_text(CONTRACT),
        "discover": ["src/**/*.py"],
        "files": {
            SOURCE: hash_text("VALUE = 1\n"),
            SUPPORT: hash_text("def adapt(value): return value\n"),
            PLAN: hash_text("strict = true\n"),
        },
        "message": "falsey value selected",
        "obligation": "first-present",
        "outcome": "pass",
    });
    Ok((temporary, receipt))
}

fn write_receipts(root: &Path, receipts: &[Value]) -> Result<()> {
    fs::write(
        root.join(RECEIPTS),
        json!({"receipts": receipts, "version": 1_i32}).to_string(),
    )?;
    Ok(())
}

#[test]
#[expect(
    clippy::indexing_slicing,
    clippy::panic_in_result_fn,
    reason = "A test reports its failure by panicking."
)]
fn verifies_only_the_exact_contract_and_dependency_bytes() -> Result<()> {
    let (temporary, receipt) = create_fixture()?;
    let root = temporary.path();
    write_receipts(root, &[receipt])?;
    let path = root.join(RECEIPTS);
    let rows = verify(root, &path)?;
    assert_eq!(rows[0].freshness, Freshness::Current);
    assert_eq!(rows[0].observed_outcome, Some(Outcome::Pass));
    assert_eq!(rows[1].freshness, Freshness::Missing);

    fs::write(root.join(SUPPORT), "def adapt(value): return None\n")?;
    let edited = verify(root, &path)?;
    assert_eq!(edited[0].freshness, Freshness::Stale);

    fs::write(root.join(SUPPORT), "def adapt(value): return value\n")?;
    let restored = verify(root, &path)?;
    assert_eq!(restored[0].freshness, Freshness::Current);

    fs::write(
        root.join(DOCUMENT),
        format!("{CONTRACT}\nReview the caller.\n"),
    )?;
    let changed_contract = verify(root, &path)?;
    assert_eq!(changed_contract[0].freshness, Freshness::Stale);
    Ok(())
}

#[test]
#[expect(
    clippy::indexing_slicing,
    clippy::panic_in_result_fn,
    reason = "A test reports its failure by panicking."
)]
fn marks_evidence_stale_when_discovery_gains_a_subject() -> Result<()> {
    let (temporary, receipt) = create_fixture()?;
    let root = temporary.path();
    write_receipts(root, &[receipt])?;
    fs::write(root.join("src/new.py"), "VALUE = 2\n")?;
    let rows = verify(root, &root.join(RECEIPTS))?;
    assert_eq!(rows[0].freshness, Freshness::Stale);
    assert_eq!(rows[0].observed_outcome, Some(Outcome::Pass));
    Ok(())
}

#[test]
#[expect(
    clippy::indexing_slicing,
    clippy::panic_in_result_fn,
    reason = "A test reports its failure by panicking."
)]
fn oversized_dependencies_leave_the_obligation_unresolved() -> Result<()> {
    let (temporary, receipt) = create_fixture()?;
    let root = temporary.path();
    write_receipts(root, &[receipt])?;
    fs::File::create(root.join(SUPPORT))?
        .set_len(u64::try_from(MAX_CAPTURE_BYTES.saturating_add(1))?)?;
    let rows = verify(root, &root.join(RECEIPTS))?;
    assert_eq!(rows[0].freshness, Freshness::Unresolved);
    assert!(
        rows[0]
            .reason
            .as_deref()
            .is_some_and(|text| text.contains("capture limit"))
    );
    Ok(())
}

#[test]
#[expect(
    clippy::indexing_slicing,
    clippy::panic_in_result_fn,
    reason = "A test reports its failure by panicking."
)]
fn keeps_counterexamples_and_unresolved_runs_visible() -> Result<()> {
    let (temporary, mut failure) = create_fixture()?;
    let root = temporary.path();
    failure["outcome"] = json!("fail");
    failure["message"] = json!("source label differs from the winning reader");
    let mut timeout = failure.clone();
    timeout["obligation"] = json!("no-fallback");
    timeout["outcome"] = json!("unresolved");
    timeout["message"] = json!("checker timed out");
    write_receipts(root, &[failure.clone(), timeout])?;
    let rows = verify(root, &root.join(RECEIPTS))?;
    assert_eq!(rows[0].observed_outcome, Some(Outcome::Fail));
    assert_eq!(rows[1].observed_outcome, Some(Outcome::Unresolved));
    assert_eq!(rows[1].message.as_deref(), Some("checker timed out"));

    let mut passing = failure.clone();
    passing["outcome"] = json!("pass");
    write_receipts(root, &[failure, passing])?;
    let Err(error) = verify(root, &root.join(RECEIPTS)) else {
        anyhow::bail!("duplicate receipt masked a counterexample");
    };
    assert!(error.to_string().contains("duplicate receipt"));
    Ok(())
}
