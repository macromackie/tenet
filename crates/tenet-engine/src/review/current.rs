//! The review as it stands in the working tree, computed without a model and without writes.
//!
//! Revisions against the same base commit form one review. Each contract is judged per path
//! selection: a revision's assessment of a contract stands until a later revision assesses that
//! contract with paths that cover it, or assesses the contract's whole scope. A contract is
//! settled only when every assessment still standing is settled, so a narrower check never hides
//! pending work from a broader one. In a diff review, every contract a checked selection governs
//! takes part, so checking one contract by name leaves the others unchecked, not gone.

use alloc::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tenet_contracts::Contract;

use super::{Assessment, Review, ReviewError, Revision, ScopedFile};
use crate::{
    Change,
    ContractState,
    Mode,
    Pending,
    RequestedScope,
    Selection,
    discover_contracts,
    evidence::ChangeError,
    git,
    plan,
};

/// No check compares the working tree with this base commit yet.
#[derive(Debug, thiserror::Error)]
#[error("no check compares the working tree with {base:?}; run tenet check --base {base:?}")]
pub struct Unchecked {
    pub base: String,
}

/// The contract filter of a selection that covers every contract.
const EVERY_CONTRACT: &Option<String> = &None;

/// The current state of every contract that a review has checked.
#[derive(Debug)]
pub struct Current<'review> {
    /// One row per contract and standing path selection, in contract order.
    pub contracts: Vec<CurrentContract<'review>>,
    /// Contract documents the change deletes; their requirements left the review.
    pub deleted: Vec<PathBuf>,
    /// The latest check; its base commit identifies the review.
    pub latest: &'review Revision,
    /// A diff review whose working tree has no reviewable change from its base.
    pub unchanged: bool,
}

/// The current state of one contract in a review.
#[derive(Debug)]
pub struct CurrentContract<'review> {
    /// The assessment of exactly this input, if a check made one.
    pub assessment: Option<&'review Assessment>,
    pub changes: Vec<Change>,
    pub contract: Contract,
    pub files: Vec<ScopedFile>,
    pub input: String,
    /// The confidence threshold that routed the model's conclusion for this input.
    pub min_confidence: Option<f64>,
    pub mode: Mode,
    pub scope: RequestedScope,
    /// The latest check of this row's path selection, recaptured to compute its input.
    pub selection: &'review Revision,
    pub state: ContractState,
}

impl<'review> Current<'review> {
    /// The contract's row for these selected paths. Without paths, its only row, or the row of a
    /// check that selected everything.
    #[inline]
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    pub fn select(
        &self,
        name: &str,
        paths: &[PathBuf],
    ) -> Result<&CurrentContract<'review>, ReviewError> {
        let rows: Vec<_> = self
            .contracts
            .iter()
            .filter(|row| row.contract.name == name)
            .collect();
        if rows.is_empty() {
            return Err(ReviewError::NotInReview(name.to_owned()));
        }
        if let [only] = rows[..]
            && paths.is_empty()
        {
            return Ok(only);
        }
        rows.iter()
            .copied()
            .find(|row| row.selection.is_selecting(paths))
            .ok_or_else(|| {
                let choices: Vec<_> = rows
                    .iter()
                    .map(|row| {
                        match &*row.selection.paths {
                            [] => "(none: every path)".to_owned(),
                            listed => {
                                listed
                                    .iter()
                                    .map(|path| path.display().to_string())
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            },
                        }
                    })
                    .collect();
                ReviewError::Ambiguous {
                    name: name.to_owned(),
                    choices: choices.join("; "),
                }
            })
    }
}

/// What one path selection contributes to the review.
#[derive(Default)]
struct Group<'review> {
    /// Contracts whose standing assessment this selection made.
    assessed_contracts: BTreeMap<&'review str, &'review Assessment>,
    /// Set when every contract in this selection's scope belongs to the review, so one that no
    /// standing assessment covers is unchecked.
    open: Option<&'review Revision>,
}

/// Capture each standing selection again the way its latest check did, then match the result
/// against the log.
///
/// A contract whose scoped inputs changed since then is pending until it is checked again. `base`
/// chooses the diff review against that commit instead of the latest check's review; without a
/// check against it, the error is `Unchecked`.
#[expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "Assessment revisions are keys of `family_revisions`, built from the same review, \
              and `index` comes from enumerating the slice it offsets."
)]
#[inline]
#[expect(
    clippy::cognitive_complexity,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
#[expect(
    clippy::future_not_send,
    reason = "The checker drives this future on one task and never moves it across threads."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
pub async fn resolve_current<'review>(
    root: &Path,
    review: &'review Review,
    base: Option<&str>,
    only: Option<&str>,
) -> Result<Current<'review>, ReviewError> {
    let latest = match base {
        Some(reference) => {
            let commit = git::commit(root, reference)?;
            (review.list_revisions().rev())
                .find(|revision| revision.base_oid.as_ref() == Some(&commit))
                .ok_or(Unchecked { base: commit })?
        },
        None => (review.list_revisions().next_back()).ok_or(ReviewError::NoChecks)?,
    };
    let family_revisions: BTreeMap<u64, &Revision> = review
        .list_revisions()
        .filter(|revision| revision.is_same_review(latest))
        .map(|revision| (revision.rev, revision))
        .collect();

    // A later assessment replaces an earlier one when it covered the contract's whole scope or
    // selected every path the earlier one did.
    let replaces = |later: &Assessment, earlier: &Revision| {
        later.scope == RequestedScope::FullContract
            || family_revisions[&later.rev].is_covering(earlier)
    };
    let mut assessed_contracts: BTreeMap<&str, Vec<&Assessment>> = BTreeMap::new();
    for assessment in review.list_assessments() {
        if family_revisions.contains_key(&assessment.rev) {
            assessed_contracts
                .entry(&assessment.contract)
                .or_default()
                .push(assessment);
        }
    }
    let mut standing_assessments: BTreeMap<&str, Vec<&Assessment>> = BTreeMap::new();
    for (name, assessments) in &assessed_contracts {
        for (index, assessment) in assessments.iter().enumerate() {
            let covered = assessments[index + 1..]
                .iter()
                .any(|later| replaces(later, family_revisions[&assessment.rev]));
            if !covered {
                standing_assessments
                    .entry(name)
                    .or_default()
                    .push(assessment);
            }
        }
    }

    // Recapture each selection once, with its contract filter and supporting context.
    let mut groups: BTreeMap<_, Group<'_>> = BTreeMap::new();
    for assessment in standing_assessments.values().flatten() {
        let revision = family_revisions[&assessment.rev];
        let _: Option<&_> = groups
            .entry((&revision.paths, &revision.contract, &revision.context))
            .or_insert_with(Group::default)
            .assessed_contracts
            .insert(&assessment.contract, assessment);
    }
    // A selection is open to every contract in its scope when its check ran them all, or in a
    // diff review, where the change's contracts belong to the review whichever one a check named.
    let open: Vec<&Revision> = family_revisions
        .values()
        .copied()
        .filter(|revision| revision.contract.is_none() || revision.base_oid.is_some())
        .collect();
    for (index, revision) in open.iter().enumerate() {
        if !open[index + 1..]
            .iter()
            .any(|later| later.is_covering(revision))
        {
            groups
                .entry((&revision.paths, EVERY_CONTRACT, &revision.context))
                .or_insert_with(Group::default)
                .open = Some(revision);
        }
    }
    // An open selection that did not assess a contract leaves it unchecked, unless a standing
    // assessment of the contract covers that selection.
    let unchecked = |name: &str, revision: &Revision| {
        !standing_assessments
            .get(name)
            .is_some_and(|list| list.iter().any(|assessment| replaces(assessment, revision)))
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
            group.assessed_contracts.contains_key(name)
                || group.open.is_some_and(|revision| unchecked(name, revision))
        })
    });

    let mut contracts = Vec::new();
    let mut deleted_paths = BTreeSet::new();
    let mut unchanged = false;
    for ((paths, filter, context), group) in groups {
        let selection = Selection {
            base: latest.base_oid.clone(),
            contract: filter.clone(),
            paths: paths.clone(),
        };
        let plan = plan(root, &selection)?.with_context(context)?;
        // Every plan compares the same base commit with the same working tree.
        deleted_paths.extend(plan.list_deleted_contracts());
        unchanged = plan.mode == Mode::Diff
            && plan.candidate_files.is_empty()
            && plan.contract_changes.is_empty();
        let evidence = plan.capture().await?;
        for contract in &plan.contracts {
            let name = contract.name.as_str();
            if only.is_some_and(|selected| selected != name) {
                continue;
            }
            let (chosen, latest_assessment) = match (group.assessed_contracts.get(name), group.open)
            {
                (Some(assessment), _) => (family_revisions[&assessment.rev], Some(*assessment)),
                (None, Some(revision)) if unchecked(name, revision) => (revision, None),
                _ => continue,
            };
            let input = evidence.derive_input_key(contract);
            let assessment = review.find_assessment(name, &input);
            let state = match (assessment, latest_assessment) {
                (Some(found), _) => {
                    review.compute_state(
                        &plan,
                        contract,
                        &input,
                        found
                            .conclusion
                            .as_ref()
                            .map(|conclusion| conclusion.status),
                    )
                },
                (None, Some(_)) => {
                    ContractState::Pending {
                        reason: Pending::InputsChanged,
                    }
                },
                (None, None) => {
                    ContractState::Pending {
                        reason: Pending::NotChecked,
                    }
                },
            };
            let answered = assessment.filter(|answer| answer.conclusion.is_some());
            let min_confidence = answered
                .map(|answer| review.find_revision(answer.carried_from.unwrap_or(answer.rev)))
                .transpose()?
                .and_then(|revision| revision.min_confidence);
            let files = evidence.list_scoped_files(contract);
            let changes = files
                .iter()
                .filter(|scoped| scoped.file.error.is_none())
                .map(|scoped| evidence.change(&scoped.file.path).cloned())
                .collect::<Result<_, ChangeError>>()?;
            contracts.push(CurrentContract {
                assessment,
                changes,
                contract: contract.clone(),
                files,
                input,
                min_confidence,
                mode: plan.mode,
                scope: plan.find_requested_scope(contract),
                selection: chosen,
                state,
            });
        }
    }
    contracts.sort_by(|left, right| {
        let key = |row: &CurrentContract<'_>| {
            let name = &row.contract;
            (
                name.scope.components().count(),
                name.scope.clone(),
                name.number,
                row.selection.rev,
            )
        };
        key(left).cmp(&key(right))
    });
    Ok(Current {
        contracts,
        deleted: deleted_paths.into_iter().collect(),
        latest,
        unchanged,
    })
}
