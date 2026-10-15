use std::cell::Cell;

use anyhow::Result;
use ev_grep_core::{Evaluator, Outcome, Uncertainty};
use tenet_contracts::Contract;

use super::context::{CombinedContext, Unavailable};
use crate::{
    CheckResult, ConclusionReason, ContractResult, ContractStatus, Event, EvidencePacket,
    FailureKind, Mode, RequestFailure, RequestedScope, Stage, Status, Summary, review::Answers,
};

pub(crate) struct Completion<'a> {
    pub evidence: &'a EvidencePacket<'a>,
    pub contract: &'a Contract,
    pub input: &'a str,
    pub answers: &'a Answers,
    pub results: &'a [CheckResult],
    pub summary: &'a Summary,
    pub requests: &'a Cell<usize>,
    pub max_requests: usize,
    pub min_confidence: f64,
    pub classify: fn(&anyhow::Error) -> FailureKind,
    pub local_requests: &'a Cell<usize>,
    /// Requests this contract's assessments needed after the budget was spent.
    pub refused: &'a Cell<usize>,
}

impl Completion<'_> {
    fn decision(
        &self,
        status: ContractStatus,
        reason_code: ConclusionReason,
        reason: &str,
    ) -> ContractResult {
        decision(
            self.evidence,
            self.contract,
            self.input,
            status,
            reason_code,
            reason,
        )
    }

    pub(crate) async fn assess(
        &self,
        prepared: Result<CombinedContext, Unavailable>,
        evaluator: &impl Evaluator,
        emit: &impl Fn(Event) -> Result<()>,
    ) -> Result<ContractResult> {
        if self.evidence.plan.untouched(self.contract) {
            let (status, reason_code, reason) = self.clear_decision();
            return Ok(self.decision(status, reason_code, &reason));
        }
        let context = match prepared {
            Ok(context) => context,
            Err(unavailable) => return Ok(self.without_combined(unavailable)),
        };
        let query = super::prompt::query(Stage::Completeness, self.evidence.plan.mode);
        let questions = [ev_grep_core::Question {
            id: "match",
            query: &query,
        }];
        let key = self.answers.key(&questions, &context.state);
        let cached = self
            .answers
            .get(&key)
            .and_then(|mut answers| answers.remove("match"));
        let assessment = if let Some(assessment) = &cached {
            assessment.clone()
        } else {
            if self.requests.get() >= self.max_requests {
                self.refused.set(self.refused.get() + 1);
                return Ok(self.without_combined(Unavailable {
                    reason_code: ConclusionReason::RequestBudget,
                    reason: "Request budget exhausted before combined assessment.".into(),
                    error: None,
                    failure: None,
                    context: Some(Box::new(context)),
                }));
            }
            self.requests.set(self.requests.get() + 1);
            self.local_requests.set(self.local_requests.get() + 1);
            emit(Event::StageStarted {
                contract: self.contract.name.clone(),
                path: "<combined>".into(),
                stage: Stage::Completeness,
            })?;
            match evaluator.assess_context(&query, &context.state).await {
                Ok(assessment) => {
                    self.answers.put(
                        &key,
                        &std::collections::BTreeMap::from([("match".into(), assessment.clone())]),
                    )?;
                    assessment
                }
                Err(error) => {
                    let failure = RequestFailure::from((self.classify)(&error));
                    if let Some(cause) = failure.stops_check() {
                        return Err(error
                            .context(format!("{cause}, while assessing {}", self.contract.name)));
                    }
                    return Ok(self.without_combined(Unavailable {
                        reason_code: ConclusionReason::AssessmentFailed,
                        reason: "Combined assessment failed.".into(),
                        error: Some(error.to_string()),
                        failure: Some(failure),
                        context: Some(Box::new(context)),
                    }));
                }
            }
        };
        let routing = assessment.route(self.min_confidence);
        emit(Event::StageCompleted {
            contract: self.contract.name.clone(),
            path: "<combined>".into(),
            stage: Stage::Completeness,
            outcome: routing.outcome,
        })?;
        let flagged = self.flagged();
        let incomplete = self.summary.errors > 0 || self.summary.incomplete > 0;
        let (status, reason_code, reason) = match routing.outcome {
            Outcome::Match => (
                ContractStatus::Failed,
                ConclusionReason::Violation,
                "The supplied evidence supports a violation in the requested scope.".into(),
            ),
            Outcome::NoMatch if !flagged.is_empty() => (
                ContractStatus::Unresolved,
                ConclusionReason::ConflictingAssessments,
                "File and combined assessments disagree; review the flagged files.".into(),
            ),
            Outcome::NoMatch if incomplete => (
                ContractStatus::Unresolved,
                ConclusionReason::FileChecksIncomplete,
                "File checks could not complete; see their errors or limits.".into(),
            ),
            Outcome::NoMatch => self.clear_decision(),
            Outcome::Uncertain if !flagged.is_empty() => (
                ContractStatus::Failed,
                ConclusionReason::FileViolation,
                format!(
                    "File checks found a violation in {}; the combined assessment was uncertain.",
                    flagged.join(", ")
                ),
            ),
            Outcome::Uncertain => (
                ContractStatus::Unresolved,
                ConclusionReason::ModelUncertain,
                match routing.reason {
                    Some(Uncertainty::LowConfidence) => {
                        "Combined assessment did not meet the configured confidence threshold."
                    }
                    Some(Uncertainty::ModelUncertain) | None => {
                        "Combined assessment could not resolve the requested scope from the supplied evidence."
                    }
                }
                .into(),
            ),
        };
        let mut result = self.decision(status, reason_code, &reason);
        result.violation = Some(crate::violation(&assessment));
        result.evidence_hash = Some(context.hash);
        result.context = Some(context.summary);
        result.request = assessment.request.clone();
        result.cached = cached.is_some();
        if reason_code == ConclusionReason::ConflictingAssessments {
            result.conflicting_files = flagged;
        }
        result.assessment = Some(assessment);
        result.routing = Some(routing);
        Ok(result)
    }

    /// Files whose checks found a violation.
    fn flagged(&self) -> Vec<String> {
        self.results
            .iter()
            .filter(|file| file.status == Status::Fail)
            .map(|file| file.path.clone())
            .collect()
    }

    /// Conclude from file checks alone: a flagged file fails the contract; otherwise it stays
    /// unresolved for the reason the combined assessment could not run.
    fn without_combined(&self, unavailable: Unavailable) -> ContractResult {
        let flagged = self.flagged();
        let mut result = if flagged.is_empty() {
            self.decision(
                ContractStatus::Unresolved,
                unavailable.reason_code,
                &unavailable.reason,
            )
        } else {
            self.decision(
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
        result.violation = self
            .results
            .iter()
            .filter_map(|file| file.verification.as_ref().map(crate::violation))
            .max_by(f64::total_cmp);
        result
    }

    fn clear_decision(&self) -> (ContractStatus, ConclusionReason, String) {
        if self.evidence.plan.requested_scope(self.contract) == RequestedScope::SelectedSubjects {
            return (
                ContractStatus::Clear,
                ConclusionReason::SelectedSubjectsClear,
                "No violation found in the selected subjects; this does not verify the full contract scope.".into(),
            );
        }
        if self.evidence.plan.mode == Mode::Diff {
            if self
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
}

pub(super) fn decision(
    evidence: &EvidencePacket<'_>,
    contract: &Contract,
    input: &str,
    status: ContractStatus,
    reason_code: ConclusionReason,
    reason: &str,
) -> ContractResult {
    ContractResult {
        status,
        scope: evidence.plan.requested_scope(contract),
        input: input.to_owned(),
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
