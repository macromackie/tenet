//! Commit captured assessments as one ordered, repeat-aware review revision.

use std::collections::HashMap;

use super::{
    Assessment,
    CheckRequest,
    Checked,
    Entry,
    Review,
    ReviewError,
    Revision,
    log,
    read_clock,
    store,
};
use crate::{EvidencePacket, Plan, git};

impl Review {
    /// Append one revision and its assessments, and keep the captured contents for later diffs.
    /// A check that repeats the latest revision's selection, inputs, and conclusions appends
    /// nothing. Returns the revision number, or `None` when this review keeps no state.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The store offset and revision number grow by one append each and stay far below \
                  the integer's limit."
    )]
    #[inline]
    #[expect(
        clippy::as_conversions,
        reason = "A usize fits in a u64 on every supported target."
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "The steps run in one order and share local state; splitting them would add \
                  forwarding functions."
    )]
    pub fn commit(
        &mut self,
        evidence: &EvidencePacket<'_>,
        request: &CheckRequest<'_>,
        checked: &Checked,
    ) -> Result<Option<u64>, ReviewError> {
        let Some(store) = &self.store else {
            return Ok(None);
        };
        let plan = evidence.plan;
        let mut files = Vec::new();
        let mut positions = HashMap::new();
        let mut assessments = Vec::new();
        for (contract, concluded) in plan.contracts.iter().zip(&checked.contracts) {
            drop(store.write_blob(&contract.document)?);
            let scoped = evidence.list_scoped_files(contract);
            let (selected, support) = log::add_files(&mut files, &mut positions, scoped);
            assessments.push(Assessment {
                rev: 0,
                contract: concluded.contract.clone(),
                contract_hash: contract.hash.clone(),
                input: concluded.input.clone(),
                scope: concluded.scope,
                conclusion: concluded.conclusion.clone(),
                carried_from: concluded.carried_from,
                selected,
                support,
                results: concluded.results.clone(),
            });
        }
        for file in files.iter().filter(|file| file.after.is_some()) {
            if let Some(after) = &evidence.change(&file.path)?.after {
                drop(store.write_blob(after)?);
            }
        }

        let selection = request.selection;
        let events = store.resolve_path(store::EVENTS);
        let mut number = 0;
        let _: u64 =
            store::append_after::<_, ReviewError>(&events, self.end, log::FORMAT, |appended| {
                self.extend(&events, appended, self.end + appended.len() as u64)?;
                let latest = self.list_revisions().next_back();
                let revision = Revision {
                    rev: latest.map_or(1, |previous| previous.rev + 1),
                    at: read_clock()?,
                    review: review_label(plan, selection.base.as_deref()),
                    base: selection.base.clone(),
                    paths: selection.paths.clone(),
                    contract: selection.contract.clone(),
                    context: request.context.to_vec(),
                    mode: plan.mode,
                    base_oid: plan.base.clone(),
                    head_oid: plan.head.clone(),
                    model: request.model.map(str::to_owned),
                    min_confidence: request.min_confidence,
                    tenet: env!("CARGO_PKG_VERSION").into(),
                    files,
                };
                if let Some(previous) = latest
                    && self.is_repeat(previous, &revision, &assessments)
                {
                    number = previous.rev;
                    return Ok(Vec::new());
                }
                number = revision.rev;
                let mut lines = vec![Entry::Revision(revision)];
                lines.extend(assessments.into_iter().map(|assessment| {
                    Entry::Assessment(Assessment {
                        rev: number,
                        ..assessment
                    })
                }));
                Ok(lines)
            })?;
        Ok(Some(number))
    }

    /// Whether a new revision would only repeat the latest one: the same selection and model, and
    /// for every contract the same input and conclusion.
    fn is_repeat(
        &self,
        latest: &Revision,
        revision: &Revision,
        assessments: &[Assessment],
    ) -> bool {
        let same_selection = latest.is_same_review(revision)
            && latest.is_selecting(&revision.paths)
            && latest.contract == revision.contract
            && latest.context == revision.context
            && latest.mode == revision.mode
            && latest.model == revision.model
            && latest.min_confidence == revision.min_confidence;
        let previous = self.list_assessments_in(latest.rev).map(|assessment| {
            (
                &assessment.contract,
                &assessment.input,
                &assessment.conclusion,
            )
        });
        same_selection
            && previous.eq(assessments.iter().map(|assessment| {
                (
                    &assessment.contract,
                    &assessment.input,
                    &assessment.conclusion,
                )
            }))
    }
}

/// The branch and the base commit the change is measured from, for display.
fn review_label(plan: &Plan, base: Option<&str>) -> String {
    let branch = git::read_branch(&plan.root)
        .filter(|branch| branch != "HEAD")
        .map(|branch| format!("{branch}: "))
        .unwrap_or_default();
    match (base, &plan.base) {
        (Some(prefix), Some(oid)) if oid.starts_with(prefix) => {
            format!("{branch}changes since {oid:.12}")
        },
        (Some(prefix), Some(oid)) => format!("{branch}changes since {prefix} ({oid:.12})"),
        _ => format!("{branch}full check"),
    }
}
