use super::{Completion, Unavailable};
use crate::{ConclusionReason, ContractResult, ContractStatus, Mode, RequestedScope, Status};

pub(super) fn clear_decision(
    completion: &Completion<'_>,
) -> (ContractStatus, ConclusionReason, String) {
    if completion
        .evidence
        .plan
        .find_requested_scope(completion.contract)
        == RequestedScope::SelectedSubjects
    {
        return (
            ContractStatus::Clear,
            ConclusionReason::SelectedSubjectsClear,
            "No violation found in the selected subjects; this does not verify the full contract \
             scope."
                .into(),
        );
    }
    if completion.evidence.plan.mode == Mode::Diff {
        if completion
            .results
            .iter()
            .all(|file| file.status == Status::NotApplicable)
        {
            return (
                ContractStatus::Unaffected,
                ConclusionReason::NoRelevantChanges,
                "No relevant changes found; base compliance is assumed.".into(),
            );
        }
        return (
            ContractStatus::Preserved,
            ConclusionReason::Compliance,
            "Evidence supports preservation, assuming base compliance.".into(),
        );
    }
    (
        ContractStatus::Verified,
        ConclusionReason::Compliance,
        "The supplied evidence supports compliance across the full contract scope.".into(),
    )
}

pub(super) fn decide(
    completion: &Completion<'_>,
    status: ContractStatus,
    reason_code: ConclusionReason,
    reason: &str,
) -> ContractResult {
    ContractResult {
        status,
        scope: completion
            .evidence
            .plan
            .find_requested_scope(completion.contract),
        input: completion.input.to_owned(),
        violation: None,
        reason: reason.into(),
        reason_code,
        context: None,
        assessment: None,
        routing: None,
        evidence_hash: None,
        error: None,
        failure: None,
        conflicting_files: Vec::new(),
        request: None,
        cached: false,
    }
}

/// Conclude from file checks alone: a flagged file fails the contract; otherwise it stays
/// unresolved for the reason the combined assessment could not run.
pub(super) fn drop_combined(
    completion: &Completion<'_>,
    unavailable: Unavailable,
) -> ContractResult {
    let flagged = list_flagged(completion);
    let mut result = if flagged.is_empty() {
        decide(
            completion,
            ContractStatus::Unresolved,
            unavailable.reason_code,
            &unavailable.reason,
        )
    } else {
        decide(
            completion,
            ContractStatus::Failed,
            ConclusionReason::FileViolation,
            &format!(
                "File checks found a violation in {}. {}",
                flagged.join(", "),
                unavailable.reason
            ),
        )
    };
    result.error = unavailable.error;
    result.failure = unavailable.failure;
    if let Some(context) = unavailable.context {
        result.context = Some(context.summary);
        result.evidence_hash = Some(context.hash);
    }
    result.violation = completion
        .results
        .iter()
        .filter_map(|file| file.verification.as_ref().map(crate::estimate_violation))
        .max_by(f64::total_cmp);
    result
}

/// Files whose checks found a violation.
pub(super) fn list_flagged(completion: &Completion<'_>) -> Vec<String> {
    completion
        .results
        .iter()
        .filter(|file| file.status == Status::Fail)
        .map(|file| file.path.clone())
        .collect()
}
