//! The review as it stands in the working tree, computed without a model and without writes.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::Serialize;
use tenet_contracts::Contract;

use super::{Assessment, Review, Revision, ScopedFile};
use crate::{
    Change, ContractState, Mode, Pending, RequestedScope, Selection, discover_contracts, plan,
};

pub struct Current {
    /// The latest check; its base and paths identify the review.
    pub latest: Revision,
    pub contracts: Vec<CurrentContract>,
}

#[derive(Serialize)]
pub struct CurrentContract {
    pub contract: Contract,
    pub mode: Mode,
    pub scope: RequestedScope,
    pub input: String,
    /// The assessment of exactly this input, if a check made one.
    pub assessment: Option<Assessment>,
    /// The review's latest assessment of this contract; outdated when its input differs.
    pub latest: Option<Assessment>,
    pub state: ContractState,
    pub files: Vec<ScopedFile>,
    pub changes: Vec<Change>,
}

/// Capture each contract again the way its latest check did, then match the result against the
/// log. A contract whose scoped inputs changed since then is pending until it is checked again.
pub async fn current(root: &Path, review: &Review, only: Option<&str>) -> Result<Current> {
    let latest = review
        .revisions()
        .last()
        .context("no checks recorded; run tenet check first")?
        .clone();
    let family: BTreeMap<u64, &Revision> = review
        .revisions()
        .filter(|revision| revision.same_review(&latest))
        .map(|revision| (revision.rev, revision))
        .collect();
    let mut checked: BTreeMap<&str, &Assessment> = BTreeMap::new();
    for assessment in review.assessments() {
        if family.contains_key(&assessment.rev) {
            checked.insert(&assessment.contract, assessment);
        }
    }

    // Recapture with each assessment's own selection: its contract filter and supporting context.
    let mut groups: BTreeMap<(Option<String>, Vec<PathBuf>), Vec<&str>> = BTreeMap::new();
    for (name, assessment) in &checked {
        let revision = family[&assessment.rev];
        groups
            .entry((revision.contract.clone(), revision.context.clone()))
            .or_default()
            .push(name);
    }
    // A review that ran without a contract filter covers every contract that enters its scope.
    let unfiltered = family.values().rev().find(|r| r.contract.is_none());
    if let Some(revision) = unfiltered {
        groups.entry((None, revision.context.clone())).or_default();
    }
    // A contract checked by name leaves the review once no contract has that name.
    if groups.keys().any(|(filter, _)| filter.is_some()) {
        let names: BTreeSet<String> = discover_contracts(root)?
            .into_iter()
            .map(|contract| contract.name)
            .collect();
        groups.retain(|(filter, _), _| filter.as_ref().is_none_or(|name| names.contains(name)));
    }
    groups.retain(|(filter, _), names| {
        only.is_none_or(|name| {
            names.contains(&name) || (filter.is_none() && !checked.contains_key(name))
        })
    });

    let mut contracts = Vec::new();
    for ((filter, context), names) in groups {
        let selection = Selection {
            paths: latest.paths.clone(),
            base: latest.base.clone(),
            contract: filter.clone(),
        };
        let plan = plan(root, &selection)?.with_context(&context)?;
        let evidence = plan.capture().await?;
        for contract in &plan.contracts {
            let name = contract.name.as_str();
            let assessed_here = names.contains(&name);
            let unchecked = filter.is_none()
                && !checked.contains_key(name)
                && unfiltered.is_some_and(|r| r.context == context);
            if !(assessed_here || unchecked) || only.is_some_and(|only| only != name) {
                continue;
            }
            let input = evidence.input_key(contract);
            let assessment = review.assessment(name, &input).cloned();
            let latest_assessment = checked.get(name).map(|a| (*a).clone());
            let state = match (&assessment, &latest_assessment) {
                (Some(assessment), _) => review.state(
                    name,
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
            let files = evidence.scoped_files(contract);
            let changes = files
                .iter()
                .filter(|file| file.error.is_none())
                .map(|file| evidence.change(&file.path).cloned())
                .collect::<Result<_>>()?;
            contracts.push(CurrentContract {
                contract: contract.clone(),
                mode: plan.mode,
                scope: plan.requested_scope(contract),
                input,
                assessment,
                latest: latest_assessment,
                state,
                files,
                changes,
            });
        }
    }
    contracts.sort_by(|a, b| {
        let key = |c: &Contract| (c.scope.components().count(), c.scope.clone(), c.number);
        key(&a.contract).cmp(&key(&b.contract))
    });
    Ok(Current { latest, contracts })
}
