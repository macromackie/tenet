mod controls;
mod workspace;

use alloc::collections::BTreeSet;
use std::{io, path::Path};

use controls::{Case, Manifest, hash, read_patches};
use serde::Serialize;
use tenet_contracts::Contract;
use tenet_engine::{Plan, Selection, plan};
use workspace::Workspace;

use super::{Binding, Freshness, Outcome, ReceiptError, Snapshot, read_current, select_checks};
use crate::mechanical::Finding;

/// One actual checker invocation compared with its qualification control.
#[derive(Debug, Serialize)]
pub struct QualificationCase {
    pub expected: Outcome,
    pub finding: Option<Finding>,
    pub fixture_tree: Option<String>,
    pub kind: String,
    pub message: String,
    pub name: String,
    pub observed: Option<Outcome>,
    pub outcome: Outcome,
    pub patch_sha256: Option<String>,
}

impl QualificationCase {
    fn new(case: &Case) -> Self {
        Self {
            expected: case.kind.require_outcome(),
            finding: None,
            fixture_tree: None,
            kind: match case.kind {
                controls::Kind::Good => "good",
                controls::Kind::Defect => "defect",
                controls::Kind::Refactor => "refactor",
            }
            .to_owned(),
            message: String::new(),
            name: case.name.clone(),
            observed: None,
            outcome: Outcome::Unresolved,
            patch_sha256: None,
        }
    }
}

/// Checker qualification against project-provided controls, independent of project review state.
#[derive(Debug, Serialize)]
pub struct Qualification {
    pub binding_sha256: Option<String>,
    pub cases: Vec<QualificationCase>,
    pub checker_sha256: Option<String>,
    pub contract_sha256: Option<String>,
    pub freshness: Freshness,
    pub manifest_sha256: Option<String>,
    pub outcome: Outcome,
    pub reason: Option<String>,
    pub selector: String,
}

async fn observe_case(
    workspace: &Workspace<'_>,
    case: &Case,
    patch: io::Result<Option<Vec<u8>>>,
    identities: &mut BTreeSet<String>,
    row: &mut QualificationCase,
) -> io::Result<()> {
    let bytes = patch?;
    row.patch_sha256 = bytes.as_deref().map(hash);
    let (checkout, tree) = workspace.prepare(bytes.as_deref())?;
    row.fixture_tree = Some(tree.clone());
    if !identities.insert(tree) {
        return Err(io::Error::other(
            "duplicate control tree; each control needs a distinct example",
        ));
    }
    let evidence = workspace.observe(checkout.read_root()).await?;
    row.observed = Some(evidence.outcome);
    row.message = evidence.message;
    row.finding = evidence.finding;
    if matches!(row.observed, Some(Outcome::Fail | Outcome::Pass)) {
        let finding = row.finding.as_ref().map(|detail| detail.case.as_str());
        row.outcome = if row.observed == Some(row.expected)
            && case
                .finding_case
                .as_deref()
                .is_none_or(|required| finding == Some(required))
        {
            Outcome::Pass
        } else {
            Outcome::Fail
        };
        if let Some(required) = &case.finding_case
            && finding != Some(required.as_str())
        {
            row.message = format!("expected finding {required:?}; {}", row.message);
        }
    }
    Ok(())
}

async fn run_cases(
    tree: &Plan,
    contract: &Contract,
    name: &str,
    report: &mut Qualification,
) -> io::Result<()> {
    let directory = tree.root.join(contract.path.with_file_name("checks"));
    let path = directory.join(format!("{name}.qualification.json"));
    let (manifest, manifest_hash) = Manifest::read(&path)?;
    report.manifest_sha256 = Some(manifest_hash.clone());
    let (binding, before) = Binding::read(tree, contract, name)?;
    report.binding_sha256 = Some(before.binding.clone());
    report.contract_sha256 = Some(before.contract.clone());
    report.checker_sha256 = Some(before.tool.clone());
    let workspace = Workspace::create(&tree.root, contract, name, &binding, &before, &manifest)?;
    let patches = read_patches(&directory, &manifest);
    let mut identities = BTreeSet::new();
    for (case, patch) in manifest.cases.iter().zip(patches) {
        let mut row = QualificationCase::new(case);
        if let Err(error) = observe_case(&workspace, case, patch, &mut identities, &mut row).await {
            row.message = error.to_string();
        }
        report.cases.push(row);
    }
    let latest = plan(&tree.root, &Selection::default()).map_err(io::Error::other)?;
    let (_, after) = read_current(&latest, contract, name)?;
    let (_, latest_manifest) = Manifest::read(&path)?;
    if let Some(reason) = find_change(&before, &after, &manifest_hash, &latest_manifest) {
        report.freshness = Freshness::Stale;
        report.reason = Some(reason);
        return Ok(());
    }
    if let Some(reason) = find_patch_change(&report.cases, &directory, &manifest)? {
        report.freshness = Freshness::Stale;
        report.reason = Some(reason);
        return Ok(());
    }
    report.freshness = Freshness::Current;
    report.outcome = if report.cases.iter().any(|row| row.outcome == Outcome::Fail) {
        Outcome::Fail
    } else if report.cases.iter().all(|row| row.outcome == Outcome::Pass) {
        Outcome::Pass
    } else {
        Outcome::Unresolved
    };
    Ok(())
}

fn find_patch_change(
    rows: &[QualificationCase],
    directory: &Path,
    manifest: &Manifest,
) -> io::Result<Option<String>> {
    for (row, patch) in rows.iter().zip(read_patches(directory, manifest)) {
        match patch {
            Ok(bytes) if bytes.as_deref().map(hash) != row.patch_sha256 => {
                return Ok(Some(format!("control {:?} changed", row.name)));
            },
            Err(error) if row.patch_sha256.is_some() => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("control {:?} became unreadable: {error:#}", row.name),
                ));
            },
            _ => {},
        }
    }
    Ok(None)
}

fn find_change(
    before: &Snapshot,
    after: &Snapshot,
    manifest: &str,
    latest: &str,
) -> Option<String> {
    if manifest != latest {
        return Some("qualification manifest changed".to_owned());
    }
    before.find_difference(after)
}

/// Run a single project checker on fresh, private copies of its declared input files.
#[inline]
pub async fn qualify(root: &Path, selector: &str) -> Result<Qualification, ReceiptError> {
    let tree = plan(root, &Selection::default())?;
    let (contract, name) = select_checks(&tree, Some(selector))?
        .into_iter()
        .next()
        .ok_or_else(|| {
            ReceiptError::Invalid {
                reason: format!("unknown local check {selector:?}"),
            }
        })?;
    let mut report = Qualification {
        binding_sha256: None,
        cases: Vec::new(),
        checker_sha256: None,
        contract_sha256: None,
        freshness: Freshness::Unresolved,
        manifest_sha256: None,
        outcome: Outcome::Unresolved,
        reason: None,
        selector: selector.to_owned(),
    };
    if let Err(error) = run_cases(&tree, contract, name, &mut report).await {
        report.reason = Some(error.to_string());
    }
    Ok(report)
}
