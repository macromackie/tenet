use crate::{
    ContractResult,
    Event,
    EvidencePacket,
    Summary,
    review::{Checked, Concluded, Review},
};

/// Reuse earlier conclusions for unchanged inputs without asking a model, reporting each contract
/// as `run` does. A reused conclusion counts as answered from an earlier check.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Counts of results, contracts and requests are bounded by the repository's files and \
              the request budget, far below the integer's limit."
)]
#[inline]
#[expect(
    clippy::impl_trait_in_params,
    reason = "Callers pass closures and evaluators whose types they cannot name."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
pub fn apply_review<Halted>(
    evidence: &EvidencePacket<'_>,
    review: &Review,
    emit: &impl Fn(Event) -> Result<(), Halted>,
) -> Result<Checked, Halted> {
    let plan = evidence.plan;
    let mut summary = Summary::default();
    let mut contracts = Vec::new();
    for contract in &plan.contracts {
        emit(Event::ContractStarted {
            contract: contract.name.clone(),
            path: contract.path.display().to_string(),
            total: plan
                .files
                .iter()
                .filter(|path| plan.is_governing(contract, path))
                .count(),
        })?;
        let input = evidence.derive_input_key(contract);
        let scope = plan.find_requested_scope(contract);
        let earlier = review
            .find_assessment(&contract.name, &input)
            .filter(|assessment| assessment.conclusion.is_some());
        let conclusion = earlier.and_then(|assessment| assessment.conclusion.clone());
        let state = review.compute_state(
            plan,
            contract,
            &input,
            conclusion.as_ref().map(|concluded| concluded.status),
        );
        let result = conclusion.as_ref().map(|concluded| {
            ContractResult {
                status: concluded.status,
                scope,
                input: input.clone(),
                violation: concluded.violation,
                reason: concluded.reason.clone(),
                reason_code: concluded.reason_code,
                context: None,
                assessment: None,
                routing: None,
                evidence_hash: None,
                error: concluded.error.clone(),
                failure: None,
                conflicting_files: concluded.conflicting_files.clone(),
                request: None,
                cached: true,
            }
        });
        summary.cached += usize::from(result.is_some());
        emit(Event::ContractFinished {
            contract: contract.name.clone(),
            summary: Summary::default(),
            conclusion: result.map(Box::new),
            state: state.clone(),
        })?;
        contracts.push(Concluded {
            carried_from: earlier
                .map(|assessment| assessment.carried_from.unwrap_or(assessment.rev)),
            conclusion,
            contract: contract.name.clone(),
            input,
            results: earlier
                .map(|assessment| assessment.results.clone())
                .unwrap_or_default(),
            scope,
            state,
        });
    }
    Ok(Checked { contracts, summary })
}
