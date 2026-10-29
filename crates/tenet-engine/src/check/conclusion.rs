use alloc::collections::BTreeMap;
use core::cell::Cell;

use ev_grep_core::{Evaluator, Outcome, Uncertainty};
use tenet_contracts::Contract;

use super::{
    CheckError,
    context::{CombinedContext, Unavailable},
};
use crate::{
    CheckResult,
    ClassifyRequest,
    ConclusionReason,
    ContractResult,
    ContractStatus,
    Event,
    EvidencePacket,
    Mode,
    RequestedScope,
    Stage,
    Status,
    Summary,
    review::Answers,
    telemetry::{self, Request},
};

pub(crate) struct Completion<'plan> {
    pub answers: &'plan Answers,
    pub contract: &'plan Contract,
    pub evidence: &'plan EvidencePacket<'plan>,
    pub input: &'plan str,
    pub local_requests: &'plan Cell<usize>,
    pub max_requests: usize,
    pub min_confidence: f64,
    /// Requests this contract's assessments needed after the budget was spent.
    pub refused: &'plan Cell<usize>,
    pub requests: &'plan Cell<usize>,
    pub results: &'plan [CheckResult],
    pub summary: &'plan Summary,
}

impl Completion<'_> {
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Counts of results, contracts and requests are bounded by the repository's files \
                  and the request budget, far below the integer's limit."
    )]
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
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
    pub(crate) async fn assess<Halted>(
        &self,
        prepared: Result<CombinedContext, Unavailable>,
        evaluator: &impl Evaluator<Error: ClassifyRequest>,
        emit: &impl Fn(Event) -> Result<(), Halted>,
    ) -> Result<ContractResult, Halted>
    where
        Halted: From<CheckError>,
    {
        if self.evidence.plan.is_untouched(self.contract) {
            let (status, reason_code, reason) = self.clear_decision();
            return Ok(self.decide(status, reason_code, &reason));
        }
        let context = match prepared {
            Ok(context) => context,
            Err(unavailable) => return Ok(self.drop_combined(unavailable)),
        };
        let query = super::prompt::query(Stage::Completeness, self.evidence.plan.mode);
        let questions = [ev_grep_core::Question {
            id: "match",
            query: &query,
        }];
        let key = Answers::derive_key(&questions, &context.state);
        let cached = self
            .answers
            .read(&key)
            .and_then(|mut answers| answers.remove("match"));
        let assessment = if let Some(assessment) = &cached {
            telemetry::record_cache_hit(Request::Combined);
            assessment.clone()
        } else {
            if self.requests.get() >= self.max_requests {
                self.refused.set(self.refused.get() + 1);
                telemetry::record_budget_exhausted(Request::Combined, self.max_requests);
                return Ok(self.drop_combined(Unavailable {
                    context: Some(Box::new(context)),
                    error: None,
                    failure: None,
                    reason: "Request budget exhausted before combined assessment.".into(),
                    reason_code: ConclusionReason::RequestBudget,
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
                    self.answers
                        .put(
                            &key,
                            &BTreeMap::from([("match".into(), assessment.clone())]),
                        )
                        .map_err(CheckError::from)?;
                    assessment
                },
                Err(error) => {
                    let failure = error.classify_request();
                    if let Some(reason) = failure.explain_stop() {
                        return Err(CheckError::CombinedRequest {
                            failure,
                            reason,
                            contract: self.contract.name.clone(),
                            source: error.into(),
                        }
                        .into());
                    }
                    return Ok(self.drop_combined(Unavailable {
                        context: Some(Box::new(context)),
                        error: Some(error.to_string()),
                        failure: Some(failure),
                        reason: "Combined assessment failed.".into(),
                        reason_code: ConclusionReason::AssessmentFailed,
                    }));
                },
            }
        };
        let routing = assessment.route(self.min_confidence);
        emit(Event::StageCompleted {
            contract: self.contract.name.clone(),
            path: "<combined>".into(),
            stage: Stage::Completeness,
            outcome: routing.outcome,
        })?;
        let flagged = self.list_flagged();
        let incomplete = self.summary.errors > 0 || self.summary.incomplete > 0;
        let (status, reason_code, reason) = match routing.outcome {
            Outcome::Match => {
                (
                    ContractStatus::Failed,
                    ConclusionReason::Violation,
                    "The supplied evidence supports a violation in the requested scope.".into(),
                )
            },
            Outcome::NoMatch if !flagged.is_empty() => {
                (
                    ContractStatus::Unresolved,
                    ConclusionReason::ConflictingAssessments,
                    "File and combined assessments disagree; review the flagged files.".into(),
                )
            },
            Outcome::NoMatch if incomplete => {
                (
                    ContractStatus::Unresolved,
                    ConclusionReason::FileChecksIncomplete,
                    "File checks could not complete; see their errors or limits.".into(),
                )
            },
            Outcome::NoMatch => self.clear_decision(),
            Outcome::Uncertain if !flagged.is_empty() => {
                (
                    ContractStatus::Failed,
                    ConclusionReason::FileViolation,
                    format!(
                        "File checks found a violation in {}; the combined assessment was \
                         uncertain.",
                        flagged.join(", ")
                    ),
                )
            },
            Outcome::Uncertain => {
                (
                    ContractStatus::Unresolved,
                    ConclusionReason::ModelUncertain,
                    match routing.reason {
                        Some(Uncertainty::LowConfidence) => {
                            "Combined assessment did not meet the configured confidence threshold."
                        },
                        Some(Uncertainty::ModelUncertain) | None => {
                            "Combined assessment could not resolve the requested scope from the \
                             supplied evidence."
                        },
                    }
                    .into(),
                )
            },
        };
        let mut result = self.decide(status, reason_code, &reason);
        result.violation = Some(crate::estimate_violation(&assessment));
        result.evidence_hash = Some(context.hash);
        result.context = Some(context.summary);
        result.request.clone_from(&assessment.request);
        result.cached = cached.is_some();
        if reason_code == ConclusionReason::ConflictingAssessments {
            result.conflicting_files = flagged;
        }
        result.assessment = Some(assessment);
        result.routing = Some(routing);
        Ok(result)
    }

    fn clear_decision(&self) -> (ContractStatus, ConclusionReason, String) {
        if self.evidence.plan.find_requested_scope(self.contract)
            == RequestedScope::SelectedSubjects
        {
            return (
                ContractStatus::Clear,
                ConclusionReason::SelectedSubjectsClear,
                "No violation found in the selected subjects; this does not verify the full \
                 contract scope."
                    .into(),
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

    fn decide(
        &self,
        status: ContractStatus,
        reason_code: ConclusionReason,
        reason: &str,
    ) -> ContractResult {
        decide(
            self.evidence,
            self.contract,
            self.input,
            status,
            reason_code,
            reason,
        )
    }

    /// Conclude from file checks alone: a flagged file fails the contract; otherwise it stays
    /// unresolved for the reason the combined assessment could not run.
    fn drop_combined(&self, unavailable: Unavailable) -> ContractResult {
        let flagged = self.list_flagged();
        let mut result = if flagged.is_empty() {
            self.decide(
                ContractStatus::Unresolved,
                unavailable.reason_code,
                &unavailable.reason,
            )
        } else {
            self.decide(
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
            .filter_map(|file| file.verification.as_ref().map(crate::estimate_violation))
            .max_by(f64::total_cmp);
        result
    }

    /// Files whose checks found a violation.
    fn list_flagged(&self) -> Vec<String> {
        self.results
            .iter()
            .filter(|file| file.status == Status::Fail)
            .map(|file| file.path.clone())
            .collect()
    }
}

pub(super) fn decide(
    evidence: &EvidencePacket<'_>,
    contract: &Contract,
    input: &str,
    status: ContractStatus,
    reason_code: ConclusionReason,
    reason: &str,
) -> ContractResult {
    ContractResult {
        status,
        scope: evidence.plan.find_requested_scope(contract),
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
