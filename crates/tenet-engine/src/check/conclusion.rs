use std::cell::Cell;

use anyhow::Result;
use ev_grep_core::{Evaluator, Outcome};
use tenet_contracts::Contract;

use crate::{
    CheckResult, ContractResult, ContractStatus, Event, Mode, Plan, Stage, Status, Summary,
};

fn decision(status: ContractStatus, reason: &str) -> ContractResult {
    ContractResult {
        status,
        confidence: None,
        reason: reason.into(),
        assessment: None,
        evidence_hash: None,
        error: None,
        conflicting_files: Vec::new(),
        request: None,
    }
}

pub(crate) struct Completion<'a> {
    pub plan: &'a Plan,
    pub contract: &'a Contract,
    pub results: &'a [CheckResult],
    pub summary: &'a Summary,
    pub requests: &'a Cell<usize>,
    pub max_requests: usize,
    pub local_requests: &'a Cell<usize>,
}

impl Completion<'_> {
    pub(crate) async fn assess(
        &self,
        evaluator: &impl Evaluator,
        emit: &impl Fn(Event) -> Result<()>,
    ) -> Result<ContractResult> {
        let diff = self.plan.mode == Mode::Diff;
        if self.summary.errors > 0 || self.summary.incomplete > 0 {
            return Ok(decision(
                ContractStatus::Unresolved,
                "File checks could not complete; see their errors or budget results.",
            ));
        }
        if self.plan.partial {
            return Ok(decision(
                ContractStatus::Unresolved,
                "Only selected paths were checked; contract-wide coverage is incomplete.",
            ));
        }
        if diff
            && self
                .results
                .iter()
                .all(|r| r.status == Status::NotApplicable)
        {
            return Ok(decision(
                ContractStatus::Unaffected,
                "No relevant changes found; base compliance is assumed.",
            ));
        }
        if self.requests.get() >= self.max_requests {
            let mut result = decision(
                ContractStatus::Unresolved,
                "Request budget exhausted before contract assessment.",
            );
            result.error = Some(result.reason.clone());
            return Ok(result);
        }
        let state = match self.evidence() {
            Ok(state) => state,
            Err(error) => {
                return Ok(decision(
                    ContractStatus::Unresolved,
                    &format!("Repository context unavailable: {error}"),
                ));
            }
        };
        let query = super::prompt::query(Stage::Completeness, self.plan.mode);
        self.requests.set(self.requests.get() + 1);
        self.local_requests.set(self.local_requests.get() + 1);
        emit(Event::StageStarted {
            contract: self.contract.name.clone(),
            path: "<repository>".into(),
            stage: Stage::Completeness,
        })?;
        let assessment = match evaluator.assess_context(&query, &state).await {
            Ok(assessment) => assessment,
            Err(error) => {
                let mut result =
                    decision(ContractStatus::Unresolved, "Repository assessment failed.");
                result.error = Some(error.to_string());
                return Ok(result);
            }
        };
        emit(Event::StageCompleted {
            contract: self.contract.name.clone(),
            path: "<repository>".into(),
            stage: Stage::Completeness,
            outcome: assessment.outcome,
        })?;
        let (status, reason) = match assessment.outcome {
            Outcome::Match => (
                ContractStatus::Failed,
                "Repository evidence supports a violation.",
            ),
            Outcome::NoMatch if diff => (
                ContractStatus::Preserved,
                "Evidence supports preservation, assuming base compliance.",
            ),
            Outcome::NoMatch => (
                ContractStatus::Verified,
                "The supplied repository evidence supports compliance.",
            ),
            Outcome::Uncertain => (
                ContractStatus::Unresolved,
                match assessment.reason {
                    Some(ev_grep_core::Uncertainty::InsufficientContext) => {
                        "Repository assessment requires context beyond the supplied scope."
                    }
                    Some(ev_grep_core::Uncertainty::LowConfidence) => {
                        "Repository assessment did not meet the model confidence threshold."
                    }
                    None => "Repository assessment could not determine compliance.",
                },
            ),
        };
        let mut result = decision(status, reason);
        result.confidence = Some(assessment.confidence);
        result.evidence_hash = Some(
            blake3::hash(&serde_json::to_vec(&state)?)
                .to_hex()
                .to_string(),
        );
        result.request = assessment.request.clone();
        if assessment.outcome == Outcome::NoMatch {
            result.conflicting_files = self
                .results
                .iter()
                .filter(|file| file.status == Status::Fail)
                .map(|file| file.path.clone())
                .collect();
            if !result.conflicting_files.is_empty() {
                result.status = ContractStatus::Unresolved;
                result.reason =
                    "File and repository assessments disagree; review the flagged files.".into();
            }
        }
        result.assessment = Some(assessment);
        Ok(result)
    }
}
