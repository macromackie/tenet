//! Convert completed checks into immutable review-log evidence.

use super::Assessed;
use crate::review::{Answer, Concluded, Conclusion, FileResult};

pub(super) fn build_concluded(assessed: &Assessed, contract: &str) -> Concluded {
    let settled = &assessed.conclusion;
    Concluded {
        carried_from: None,
        conclusion: Some(Conclusion {
            status: settled.status,
            reason_code: settled.reason_code,
            reason: settled.reason.clone(),
            violation: settled.violation,
            answer: settled.assessment.as_ref().map(|answer| {
                Answer {
                    choice: answer.choice,
                    confidence: answer.confidence,
                }
            }),
            error: settled.error.clone(),
            conflicting_files: settled.conflicting_files.clone(),
            combined_status: settled.combined_status,
            context: settled.context.clone(),
            evidence_hash: settled.evidence_hash.clone(),
            failure: settled.failure,
        }),
        contract: contract.to_owned(),
        input: settled.input.clone(),
        results: assessed
            .results
            .iter()
            .map(|file_result| {
                FileResult {
                    shared_request: file_result.shared_request.clone(),
                    path: file_result.path.clone(),
                    status: file_result.status,
                    violation: file_result
                        .verification
                        .as_ref()
                        .map(crate::estimate_violation),
                    reason: file_result.reason.clone(),
                    failure: file_result.failure,
                    omitted: file_result.omitted.clone(),
                    hunks: file_result.hunks.clone(),
                }
            })
            .collect(),
        scope: settled.scope,
        state: assessed.state.clone(),
    }
}
