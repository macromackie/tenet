//! The review as it stands in the working tree, computed without a model and without writes.
//!
//! Revisions against the same base commit form one review. Each contract is judged per path
//! selection: a revision's assessment of a contract stands until a later revision assesses that
//! contract with paths that cover it, or assesses the contract's whole scope. A contract is
//! settled only when every assessment still standing is settled, so a narrower check never hides
//! pending work from a broader one. In a diff review, every contract a checked selection governs
//! takes part, so checking one contract by name leaves the others unchecked, not gone.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use tenet_contracts::Contract;

use super::{Assessment, Review, Revision, ScopedFile};
use crate::{
    Change, ContractState, Mode, Pending, RequestedScope, Selection, discover_contracts, git, plan,
};

/// No check compares the working tree with this base commit yet.
#[derive(Debug)]
pub struct Unchecked {
    pub base: String,
}

impl std::fmt::Display for Unchecked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no check compares the working tree with {}; run tenet check --base {}",
            &self.base[..12],
            self.base
        )
    }
}

impl std::error::Error for Unchecked {}

/// The contract filter of a selection that covers every contract.
const EVERY_CONTRACT: &Option<String> = &None;

pub struct Current<'a> {
    /// The latest check; its base commit identifies the review.
    pub latest: &'a Revision,
    /// One row per contract and standing path selection, in contract order.
    pub contracts: Vec<CurrentContract<'a>>,
    /// Contract documents the change deletes; their requirements left the review.
    pub deleted: Vec<PathBuf>,
    /// A diff review whose working tree has no reviewable change from its base.
    pub unchanged: bool,
}

pub struct CurrentContract<'a> {
    pub contract: Contract,
    /// The latest check of this row's path selection, recaptured to compute its input.
    pub selection: &'a Revision,
    pub mode: Mode,
    pub scope: RequestedScope,
    pub input: String,
    /// The assessment of exactly this input, if a check made one.
    pub assessment: Option<&'a Assessment>,
    /// The confidence threshold that routed the model's conclusion for this input.
    pub min_confidence: Option<f64>,
    pub state: ContractState,
    pub files: Vec<ScopedFile>,
    pub changes: Vec<Change>,
}

impl<'a> Current<'a> {
    /// The contract's row for these selected paths. Without paths, its only row, or the row of a
    /// check that selected everything.
    pub fn select(&self, name: &str, paths: &[PathBuf]) -> Result<&CurrentContract<'a>> {
        let rows: Vec<_> = self
            .contracts
            .iter()
            .filter(|row| row.contract.name == name)
            .collect();
        ensure!(
            !rows.is_empty(),
            "{name} is not part of the current review; see tenet status"
        );
        if let [only] = rows[..]
            && paths.is_empty()
        {
            return Ok(only);
        }
        rows.iter()
            .copied()
            .find(|row| row.selection.selects(paths))
            .with_context(|| {
                let choices: Vec<_> = rows
                    .iter()
                    .map(|row| match &row.selection.paths[..] {
                        [] => "(none: every path)".to_owned(),
                        paths => paths
                            .iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>()
                            .join(" "),
                    })
                    .collect();
                format!(
                    "{name} stands in several checks of this review; name the paths of one: {}",
                    choices.join("; ")
                )
            })
    }
}

/// What one path selection contributes to the review.
#[derive(Default)]
struct Group<'a> {
    /// Contracts whose standing assessment this selection made.
    assessed: BTreeMap<&'a str, &'a Assessment>,
    /// Set when every contract in this selection's scope belongs to the review, so one that no
    /// standing assessment covers is unchecked.
    open: Option<&'a Revision>,
}

/// Capture each standing selection again the way its latest check did, then match the result
/// against the log. A contract whose scoped inputs changed since then is pending until it is
/// checked again. `base` chooses the diff review against that commit instead of the latest
/// check's review; without a check against it, the error is `Unchecked`.
pub async fn current<'a>(
    root: &Path,
    review: &'a Review,
    base: Option<&str>,
    only: Option<&str>,
) -> Result<Current<'a>> {
    let latest = match base {
        Some(reference) => {
            let base = git::commit(root, reference)?;
            (review.revisions().rev())
                .find(|revision| revision.base_oid.as_ref() == Some(&base))
                .ok_or(Unchecked { base })?
        }
        None => {
            (review.revisions().next_back()).context("no checks recorded; run tenet check first")?
        }
    };
    let family: BTreeMap<u64, &Revision> = review
        .revisions()
        .filter(|revision| revision.same_review(latest))
        .map(|revision| (revision.rev, revision))
        .collect();

    // A later assessment replaces an earlier one when it covered the contract's whole scope or
    // selected every path the earlier one did.
    let replaces = |later: &Assessment, earlier: &Revision| {
        later.scope == RequestedScope::FullContract || family[&later.rev].covers(earlier)
    };
    let mut assessed: BTreeMap<&str, Vec<&Assessment>> = BTreeMap::new();
    for assessment in review.assessments() {
        if family.contains_key(&assessment.rev) {
            assessed
                .entry(&assessment.contract)
                .or_default()
                .push(assessment);
        }
    }
    let mut standing: BTreeMap<&str, Vec<&Assessment>> = BTreeMap::new();
    for (name, assessments) in &assessed {
        for (index, assessment) in assessments.iter().enumerate() {
            let covered = assessments[index + 1..]
                .iter()
                .any(|later| replaces(later, family[&assessment.rev]));
            if !covered {
                standing.entry(name).or_default().push(assessment);
            }
        }
    }

    // Recapture each selection once, with its contract filter and supporting context.
    let mut groups: BTreeMap<_, Group> = BTreeMap::new();
    for assessment in standing.values().flatten() {
        let revision = family[&assessment.rev];
        groups
            .entry((&revision.paths, &revision.contract, &revision.context))
            .or_insert_with(Group::default)
            .assessed
            .insert(&assessment.contract, assessment);
    }
    // A selection is open to every contract in its scope when its check ran them all, or in a
    // diff review, where the change's contracts belong to the review whichever one a check named.
    let open: Vec<&Revision> = family
        .values()
        .copied()
        .filter(|revision| revision.contract.is_none() || revision.base_oid.is_some())
        .collect();
    for (index, revision) in open.iter().enumerate() {
        if !open[index + 1..].iter().any(|later| later.covers(revision)) {
            groups
                .entry((&revision.paths, EVERY_CONTRACT, &revision.context))
                .or_insert_with(Group::default)
                .open = Some(revision);
        }
    }
    // An open selection that did not assess a contract leaves it unchecked, unless a standing
    // assessment of the contract covers that selection.
    let unchecked = |name: &str, open: &Revision| {
        !standing
            .get(name)
            .is_some_and(|list| list.iter().any(|a| replaces(a, open)))
    };
    // A contract checked by name leaves the review once no contract has that name.
    if groups.keys().any(|(_, filter, _)| filter.is_some()) {
        let names: BTreeSet<String> = discover_contracts(root)?
            .into_iter()
            .map(|contract| contract.name)
            .collect();
        groups.retain(|(_, filter, _), _| filter.as_ref().is_none_or(|name| names.contains(name)));
    }
    groups.retain(|_, group| {
        only.is_none_or(|name| {
            group.assessed.contains_key(name)
                || group.open.is_some_and(|open| unchecked(name, open))
        })
    });

    let mut contracts = Vec::new();
    let mut deleted = BTreeSet::new();
    let mut unchanged = false;
    for ((paths, filter, context), group) in groups {
        let selection = Selection {
            paths: paths.clone(),
            base: latest.base_oid.clone(),
            contract: filter.clone(),
        };
        let plan = plan(root, &selection)?.with_context(context)?;
        // Every plan compares the same base commit with the same working tree.
        deleted.extend(plan.deleted_contracts());
        unchanged = plan.mode == Mode::Diff
            && plan.candidate_files.is_empty()
            && plan.contract_changes.is_empty();
        let evidence = plan.capture().await?;
        for contract in &plan.contracts {
            let name = contract.name.as_str();
            if only.is_some_and(|only| only != name) {
                continue;
            }
            let (selection, latest_assessment) = match (group.assessed.get(name), group.open) {
                (Some(assessment), _) => (family[&assessment.rev], Some(*assessment)),
                (None, Some(open)) if unchecked(name, open) => (open, None),
                _ => continue,
            };
            let input = evidence.input_key(contract);
            let assessment = review.assessment(name, &input);
            let state = match (assessment, latest_assessment) {
                (Some(assessment), _) => review.state(
                    &plan,
                    contract,
                    &input,
                    assessment.conclusion.as_ref().map(|c| c.status),
                ),
                (None, Some(_)) => ContractState::Pending {
                    reason: Pending::InputsChanged,
                },
                (None, None) => ContractState::Pending {
                    reason: Pending::NotChecked,
                },
            };
            let answered = assessment.filter(|a| a.conclusion.is_some());
            let min_confidence = answered
                .map(|a| review.revision(a.carried_from.unwrap_or(a.rev)))
                .transpose()?
                .and_then(|revision| revision.min_confidence);
            let files = evidence.scoped_files(contract);
            let changes = files
                .iter()
                .filter(|scoped| scoped.file.error.is_none())
                .map(|scoped| evidence.change(&scoped.file.path).cloned())
                .collect::<Result<_>>()?;
            contracts.push(CurrentContract {
                contract: contract.clone(),
                selection,
                mode: plan.mode,
                scope: plan.requested_scope(contract),
                input,
                assessment,
                min_confidence,
                state,
                files,
                changes,
            });
        }
    }
    contracts.sort_by(|a, b| {
        let key = |row: &CurrentContract| {
            let c = &row.contract;
            (
                c.scope.components().count(),
                c.scope.clone(),
                c.number,
                row.selection.rev,
            )
        };
        key(a).cmp(&key(b))
    });
    Ok(Current {
        latest,
        contracts,
        deleted: deleted.into_iter().collect(),
        unchanged,
    })
}
