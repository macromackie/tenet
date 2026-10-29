use core::cell::Cell;
use std::{path::Path, time::Instant};

use ev_grep_core::{Evaluator, Outcome};
use tenet_contracts::Contract;

use super::CheckError;
use crate::{
    CheckResult,
    ClassifyRequest,
    Event,
    EvidencePacket,
    Stage,
    Status,
    evidence::{InputError, hash_content},
    report::Causes,
    review::{Answers, StoreError},
    telemetry::{self, Request},
};

/// Why one file's check produced no answer; its result reports it.
#[derive(Debug, thiserror::Error)]
enum FileError {
    #[error(transparent)]
    Cache(#[from] StoreError),
    #[error(transparent)]
    Encode(#[from] serde_json::Error),
    #[error(transparent)]
    Input(#[from] InputError),
    #[error("missing {0:?} answer")]
    MissingAnswer(&'static str),
    #[error(transparent)]
    Request(anyhow::Error),
}

/// Why judging a file ended early: a failure of the file's own check, or one that ends the check.
enum Halt<Halted> {
    Check(Halted),
    File(FileError),
}

impl<Halted> From<FileError> for Halt<Halted> {
    fn from(error: FileError) -> Self {
        Self::File(error)
    }
}

#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "The sibling modules fill these fields, and the type stays private to its parent \
              module."
)]
pub(super) struct Context<'plan, Assessor, Emit> {
    pub(super) answers: &'plan Answers,
    pub(super) contract: &'plan Contract,
    pub(super) emit: &'plan Emit,
    pub(super) evaluator: &'plan Assessor,
    pub(super) evidence: &'plan EvidencePacket<'plan>,
    pub(super) local_requests: &'plan Cell<usize>,
    pub(super) max_requests: usize,
    pub(super) min_confidence: f64,
    pub(super) refused: &'plan Cell<usize>,
    pub(super) requests: &'plan Cell<usize>,
}

#[expect(
    clippy::future_not_send,
    reason = "The checker drives this future on one task and never moves it across threads."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
pub(super) async fn assess<Assessor, Emit, Halted>(
    context: &Context<'_, Assessor, Emit>,
    path: &Path,
) -> Result<CheckResult, Halted>
where
    Assessor: Evaluator<Error: ClassifyRequest>,
    Emit: Fn(Event) -> Result<(), Halted>,
    Halted: From<CheckError>,
{
    let started = Instant::now();
    let contract = context.contract;
    let mode = context.evidence.plan.mode;
    let mut result = CheckResult {
        mode,
        change_kind: None,
        previous_path: None,
        before_hash: None,
        after_hash: None,
        contract: contract.name.clone(),
        contract_path: contract.path.display().to_string(),
        contract_hash: contract.hash.clone(),
        path: path.display().to_string(),
        input_bytes: None,
        omitted: Vec::new(),
        status: Status::Error,
        message: contract.message.clone(),
        reason: None,
        failure: None,
        applicability: None,
        applicability_routing: None,
        verification: None,
        verification_routing: None,
        request: None,
        cached: false,
        elapsed_ms: 0,
    };
    (context.emit)(Event::CheckStarted {
        contract: contract.name.clone(),
        path: result.path.clone(),
    })?;
    let judged = match context.evidence.change(path) {
        Ok(change) => {
            result.change_kind = Some(change.kind);
            result.previous_path = change
                .previous_path
                .as_ref()
                .map(|previous| previous.display().to_string());
            result.before_hash = change.before.as_deref().map(hash_content);
            result.after_hash = change.after.as_deref().map(hash_content);
            judge(context, change, &mut result).await
        },
        Err(error) => Err(Halt::File(FileError::Input(error.into()))),
    };
    match judged {
        Ok(()) => {},
        Err(Halt::Check(error)) => return Err(error),
        Err(Halt::File(FileError::Input(error))) => {
            let (status, reason) = crate::classify_input_status(&error);
            result.status = status;
            result.reason = Some(reason);
        },
        Err(Halt::File(
            error @ (FileError::Request(_)
            | FileError::Encode(_)
            | FileError::Cache(_)
            | FileError::MissingAnswer(_)),
        )) => {
            result.status = Status::Error;
            result.reason = Some(Causes(&error).to_string());
        },
    }
    result.elapsed_ms = started.elapsed().as_millis();
    Ok(result)
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "Counts of results, contracts and requests are bounded by the repository's files and \
              the request budget, far below the integer's limit."
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
async fn judge<Assessor, Emit, Halted>(
    context: &Context<'_, Assessor, Emit>,
    change: &crate::Change,
    result: &mut CheckResult,
) -> Result<(), Halt<Halted>>
where
    Assessor: Evaluator<Error: ClassifyRequest>,
    Emit: Fn(Event) -> Result<(), Halted>,
    Halted: From<CheckError>,
{
    let (state, omitted) = (context.evidence)
        .build_file_input(context.contract, &change.path)
        .map_err(FileError::Input)?;
    result.input_bytes = Some(serde_json::to_vec(&state).map_err(FileError::Encode)?.len());
    result.omitted = omitted;
    let relevance = super::prompt::query(Stage::Applicability, result.mode);
    let violation = super::prompt::query(Stage::Verification, result.mode);
    let questions = [
        ev_grep_core::Question {
            id: "relevance",
            query: &relevance,
        },
        ev_grep_core::Question {
            id: "violation",
            query: &violation,
        },
    ];
    let key = Answers::derive_key(&questions, &state);
    let mut answers = if let Some(answers) = context.answers.read(&key) {
        result.cached = true;
        telemetry::record_cache_hit(Request::File);
        answers
    } else {
        if context.requests.get() >= context.max_requests {
            context.refused.set(context.refused.get() + 1);
            telemetry::record_budget_exhausted(Request::File, context.max_requests);
            result.status = Status::Incomplete;
            result.reason = Some("request budget exhausted".into());
            return Ok(());
        }
        context.requests.set(context.requests.get() + 1);
        context.local_requests.set(context.local_requests.get() + 1);
        (context.emit)(Event::StageStarted {
            contract: context.contract.name.clone(),
            path: result.path.clone(),
            stage: Stage::Applicability,
        })
        .map_err(Halt::Check)?;
        let batch = match context.evaluator.assess_questions(&questions, &state).await {
            Ok(batch) => batch,
            Err(error) => {
                let failure = error.classify_request();
                result.failure = Some(failure);
                if let Some(reason) = failure.explain_stop() {
                    return Err(Halt::Check(
                        CheckError::FileRequest {
                            failure,
                            reason,
                            path: result.path.clone(),
                            contract: context.contract.name.clone(),
                            source: error.into(),
                        }
                        .into(),
                    ));
                }
                return Err(FileError::Request(error.into()).into());
            },
        };
        (context.answers)
            .put(&key, &batch.answers)
            .map_err(FileError::Cache)?;
        result.request = Some(batch.request);
        batch.answers
    };
    let applicability = answers
        .remove("relevance")
        .ok_or(FileError::MissingAnswer("relevance"))?;
    let verification = answers
        .remove("violation")
        .ok_or(FileError::MissingAnswer("violation"))?;
    let applicability_routing = applicability.route(context.min_confidence);
    let verification_routing = verification.route(context.min_confidence);
    for (stage, routing) in [
        (Stage::Applicability, &applicability_routing),
        (Stage::Verification, &verification_routing),
    ] {
        (context.emit)(Event::StageCompleted {
            contract: context.contract.name.clone(),
            path: result.path.clone(),
            stage,
            outcome: routing.outcome,
        })
        .map_err(Halt::Check)?;
    }
    result.status = match verification_routing.outcome {
        Outcome::Match => Status::Fail,
        _ if applicability_routing.outcome == Outcome::NoMatch => Status::NotApplicable,
        Outcome::NoMatch => Status::Pass,
        Outcome::Uncertain => Status::Uncertain,
    };
    result.applicability = Some(applicability);
    result.applicability_routing = Some(applicability_routing);
    result.verification = Some(verification);
    result.verification_routing = Some(verification_routing);
    Ok(())
}
