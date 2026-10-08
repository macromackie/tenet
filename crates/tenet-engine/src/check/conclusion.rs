use std::cell::Cell;

use anyhow::Result;
use ev_grep_core::{Evaluator, Outcome, Uncertainty};
use tenet_contracts::Contract;

use crate::{
    CheckResult, ConclusionReason, ContextSummary, ContractResult, ContractStatus, Event, Mode,
    Plan, RequestedScope, Stage, Status, Summary,
};

pub(crate) struct Completion<'a> {
    pub plan: &'a Plan,
    pub contract: &'a Contract,
    pub results: &'a [CheckResult],
    pub summary: &'a Summary,
    pub requests: &'a Cell<usize>,
    pub max_requests: usize,
    pub min_confidence: f64,
    pub local_requests: &'a Cell<usize>,
}

impl Completion<'_> {
    fn decision(
        &self,
        status: ContractStatus,
        reason_code: ConclusionReason,
        reason: &str,
    ) -> ContractResult {
        ContractResult {
            status,
            scope: self.plan.requested_scope(self.contract),
            confidence: None,
            reason: reason.into(),
            reason_code,
            context: None,
            assessment: None,
            routing: None,
            evidence_hash: None,
            error: None,
            conflicting_files: Vec::new(),
            request: None,
        }
    }

    pub(crate) async fn assess(
        &self,
        evaluator: &impl Evaluator,
        emit: &impl Fn(Event) -> Result<()>,
    ) -> Result<ContractResult> {
        if self.summary.errors > 0 || self.summary.incomplete > 0 {
            return Ok(self.decision(
                ContractStatus::Unresolved,
                ConclusionReason::FileChecksIncomplete,
                "File checks could not complete; see their errors or budget results.",
            ));
        }
        if self.requests.get() >= self.max_requests {
            return Ok(self.decision(
                ContractStatus::Unresolved,
                ConclusionReason::RequestBudget,
                "Request budget exhausted before combined assessment.",
            ));
        }
        let state = match self.evidence() {
            Ok(state) => state,
            Err(error) => {
                let limit = error.is::<crate::change::InputLimit>();
                let mut result = self.decision(
                    ContractStatus::Unresolved,
                    if limit {
                        ConclusionReason::ContextTooLarge
                    } else {
                        ConclusionReason::ContextUnavailable
                    },
                    &format!("Combined context unavailable: {error}"),
                );
                if !limit {
                    result.error = Some(error.to_string());
                }
                return Ok(result);
            }
        };
        let encoded = serde_json::to_vec(&state)?;
        let evidence_hash = blake3::hash(&encoded).to_hex().to_string();
        let context = ContextSummary {
            input_bytes: encoded.len(),
            limit_bytes: ev_grep_core::MAX_FILE_BYTES,
            included_files: state["files"].as_array().map_or(0, Vec::len),
            selected_files: serde_json::from_value(state["selected_files"].clone())?,
            support_files: serde_json::from_value(state["support_files"].clone())?,
            omitted_files: serde_json::from_value(state["omitted_source"].clone())?,
        };
        let query = super::prompt::query(Stage::Completeness, self.plan.mode);
        self.requests.set(self.requests.get() + 1);
        self.local_requests.set(self.local_requests.get() + 1);
        emit(Event::StageStarted {
            contract: self.contract.name.clone(),
            path: "<combined>".into(),
            stage: Stage::Completeness,
        })?;
        let assessment = match evaluator.assess_context(&query, &state).await {
            Ok(assessment) => assessment,
            Err(error) => {
                let mut result = self.decision(
                    ContractStatus::Unresolved,
                    ConclusionReason::AssessmentFailed,
                    "Combined assessment failed.",
                );
                result.error = Some(error.to_string());
                result.evidence_hash = Some(evidence_hash);
                result.context = Some(context);
                return Ok(result);
            }
        };
        let routing = assessment.route(self.min_confidence);
        emit(Event::StageCompleted {
            contract: self.contract.name.clone(),
            path: "<combined>".into(),
            stage: Stage::Completeness,
            outcome: routing.outcome,
        })?;
        let (status, reason_code, reason) = match routing.outcome {
            Outcome::Match => (
                ContractStatus::Failed,
                ConclusionReason::Violation,
                "The supplied evidence supports a violation in the requested scope.",
            ),
            Outcome::NoMatch => self.clear_decision(),
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
                },
            ),
        };
        let mut result = self.decision(status, reason_code, reason);
        result.confidence = Some(assessment.confidence);
        result.evidence_hash = Some(evidence_hash);
        result.context = Some(context);
        result.request = assessment.request.clone();
        if routing.outcome == Outcome::NoMatch {
            result.conflicting_files = self
                .results
                .iter()
                .filter(|file| file.status == Status::Fail)
                .map(|file| file.path.clone())
                .collect();
            if !result.conflicting_files.is_empty() {
                result.status = ContractStatus::Unresolved;
                result.reason_code = ConclusionReason::ConflictingAssessments;
                result.reason =
                    "File and combined assessments disagree; review the flagged files.".into();
            }
        }
        result.assessment = Some(assessment);
        result.routing = Some(routing);
        Ok(result)
    }

    fn clear_decision(&self) -> (ContractStatus, ConclusionReason, &'static str) {
        if self.plan.requested_scope(self.contract) == RequestedScope::SelectedSubjects {
            return (
                ContractStatus::Clear,
                ConclusionReason::SelectedSubjectsClear,
                "No violation found in the selected subjects; this does not verify the full contract scope.",
            );
        }
        if self.plan.mode == Mode::Diff {
            if self
                .results
                .iter()
                .all(|file| file.status == Status::NotApplicable)
            {
                return (
                    ContractStatus::Unaffected,
                    ConclusionReason::NoRelevantChanges,
                    "No relevant changes found; base compliance is assumed.",
                );
            }
            return (
                ContractStatus::Preserved,
                ConclusionReason::Compliance,
                "Evidence supports preservation, assuming base compliance.",
            );
        }
        (
            ContractStatus::Verified,
            ConclusionReason::Compliance,
            "The supplied evidence supports compliance across the full contract scope.",
        )
    }
}
