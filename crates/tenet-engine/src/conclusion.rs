use std::cell::Cell;

use anyhow::{Result, ensure};
use ev_grep_core::{Evaluator, MAX_FILE_BYTES, Outcome};
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
    }
}

pub(crate) struct Completion<'a> {
    pub plan: &'a Plan,
    pub contract: &'a Contract,
    pub results: &'a [CheckResult],
    pub summary: &'a Summary,
    pub requests: &'a Cell<usize>,
    pub max_requests: usize,
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
        let question = if diff {
            "Assume the base satisfied this requirement. Does the change break it? Use match only for a supported violation introduced by the change, no_match only when the evidence is sufficient to establish preservation, and uncertain when evidence is missing. Account for deletions, renames, and interactions across files."
        } else {
            "Does the resulting repository violate this requirement? There is no compliance assumption. Check missing required files and cross-file requirements as well as file evidence. Use match for a supported violation, no_match only when the supplied evidence is sufficient to establish compliance, and uncertain otherwise. An empty set of findings alone is not evidence of compliance."
        };
        let query = format!(
            "{question}\nRead the whole contract, including exceptions. Behavioral rules apply when the described behavior exists; do not infer a requirement to add an absent feature. Checks describe reviewer actions, not commands that have run. Inventory lists all scoped files; supplied source contains relevant files and context. Earlier assessments are hypotheses, not proof: combined source may clear a suspected violation. Use uncertain if an obligation needs information absent from the supplied source."
        );
        self.requests.set(self.requests.get() + 1);
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
        result.assessment = Some(assessment);
        Ok(result)
    }

    fn evidence(&self) -> Result<serde_json::Value> {
        let inventory: Vec<_> = self
            .plan
            .inventory
            .iter()
            .filter(|p| p.starts_with(&self.contract.scope))
            .collect();
        let changes: Vec<_> = if self.plan.mode == Mode::Diff {
            self.plan
                .files
                .iter()
                .filter(|p| self.plan.in_scope(p, &self.contract.scope))
                .map(|p| self.plan.change(p))
                .collect::<Result<_>>()?
        } else {
            Vec::new()
        };
        let mut files = Vec::new();
        let mut bytes = 0;
        let mut omitted = Vec::new();
        for path in &inventory {
            if changes.iter().any(|change| &change.path == *path) {
                continue;
            }
            if self.plan.context.iter().any(|change| &change.path == *path) {
                continue;
            }

            let required = self.plan.mode == Mode::Full
                && !self.results.iter().any(|result| {
                    result.path == path.to_string_lossy() && result.status == Status::NotApplicable
                });
            let content = match crate::change::text(&self.plan.root, path) {
                Ok(content) if bytes + content.len() <= MAX_FILE_BYTES => content,
                _ if !required => {
                    omitted.push(path);
                    continue;
                }
                result => result?,
            };
            bytes += content.len();
            ensure!(
                bytes <= MAX_FILE_BYTES,
                "scoped repository exceeds 64 KiB; context was not truncated"
            );
            files.push(serde_json::json!({"path": path, "contents": content}));
        }
        let evidence: Vec<_> = self
            .results
            .iter()
            .map(|r| serde_json::json!({"path":r.path, "status":r.status, "reason":r.reason}))
            .collect();
        let state = serde_json::json!({"contract":self.contract.body,"inventory": inventory, "files": files, "changes": changes, "context": self.plan.context, "file_assessments":evidence, "omitted_source":omitted});
        let text = serde_json::to_vec(&state)?;
        ensure!(
            text.len() <= MAX_FILE_BYTES,
            "encoded repository context exceeds 64 KiB; context was not truncated"
        );
        Ok(state)
    }
}
