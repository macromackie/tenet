mod error;
mod hunks;

use core::cell::Cell;
use std::{path::Path, time::Instant};

use error::{FileError, Halt, set_file_error};
use ev_grep_core::{Evaluator, Outcome};
use tenet_contracts::Contract;

use super::CheckError;
use crate::{
    Change,
    CheckResult,
    ClassifyRequest,
    Event,
    EvidencePacket,
    Stage,
    Status,
    evidence::{InputError, hash_content},
    review::Answers,
    telemetry::{self, Request},
};

pub(super) fn build_result(
    contract: &Contract,
    path: &Path,
    mode: crate::Mode,
    input_limit_bytes: usize,
) -> CheckResult {
    CheckResult {
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
        input_limit_bytes,
        evidence_hash: None,
        omitted: Vec::new(),
        hunks: Vec::new(),
        hunk_summary: None,
        status: Status::Error,
        message: contract.message.clone(),
        reason: None,
        failure: None,
        applicability: None,
        applicability_routing: None,
        verification: None,
        verification_routing: None,
        request: None,
        shared_request: None,
        requests: 0,
        cached: false,
        elapsed_ms: 0,
    }
}

pub(super) fn set_change(result: &mut CheckResult, change: &Change) {
    result.change_kind = Some(change.kind);
    result.previous_path = change
        .previous_path
        .as_ref()
        .map(|previous| previous.display().to_string());
    result.before_hash = change.before.as_deref().map(hash_content);
    result.after_hash = change.after.as_deref().map(hash_content);
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
pub(super) async fn assess<Assessor, Emit, Halted>(
    context: &Context<'_, Assessor, Emit>,
    path: &Path,
) -> Result<CheckResult, Halted>
where
    Assessor: Evaluator<Error: ClassifyRequest>,
    Emit: Fn(Event) -> Result<(), Halted>,
    Halted: From<CheckError<Assessor::Error>>,
{
    let started = Instant::now();
    let contract = context.contract;
    let mode = context.evidence.plan.mode;
    let mut result = build_result(
        contract,
        path,
        mode,
        context.evidence.plan.read_input_limit_bytes(),
    );
    (context.emit)(Event::CheckStarted {
        contract: contract.name.clone(),
        path: result.path.clone(),
    })?;
    let judged = match context.evidence.change(path) {
        Ok(change) => {
            set_change(&mut result, change);
            judge(context, change, &mut result).await
        },
        Err(error) => Err(Halt::File(FileError::Input(error.into()))),
    };
    match judged {
        Ok(()) => {},
        Err(Halt::Check(error)) => return Err(error),
        Err(Halt::File(error)) => set_file_error(error, &mut result),
    }
    result.elapsed_ms = started.elapsed().as_millis();
    Ok(result)
}

#[expect(
    clippy::future_not_send,
    reason = "The checker drives this future on one task."
)]
// jscpd:ignore-start -- duplication -- Rust bounds retain evaluator and cancellation errors.
async fn judge<Assessor, Emit, Halted>(
    context: &Context<'_, Assessor, Emit>,
    change: &Change,
    result: &mut CheckResult,
) -> Result<(), Halt<Halted, Assessor::Error>>
where
    Assessor: Evaluator<Error: ClassifyRequest>,
    Emit: Fn(Event) -> Result<(), Halted>,
    Halted: From<CheckError<Assessor::Error>>,
    // jscpd:ignore-end
{
    let (state, omitted) = match context
        .evidence
        .build_file_input(context.contract, &change.path)
    {
        Ok(input) => input,
        Err(InputError::Limit(_))
            if result.mode == crate::Mode::Diff && change.before != change.after =>
        {
            return hunks::assess(context, change, result).await;
        },
        Err(error) => return Err(FileError::Input(error).into()),
    };
    result.omitted = omitted;
    judge_input(context, &state, result).await
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "Request counts are bounded by the shared request budget."
)]
#[expect(
    clippy::future_not_send,
    reason = "The checker drives this future on one task."
)]
#[expect(
    clippy::too_many_lines,
    reason = "Cache lookup, bounded dispatch and answer routing share one request lifecycle."
)]
async fn judge_input<Assessor, Emit, Halted>(
    context: &Context<'_, Assessor, Emit>,
    state: &serde_json::Value,
    result: &mut CheckResult,
) -> Result<(), Halt<Halted, Assessor::Error>>
where
    Assessor: Evaluator<Error: ClassifyRequest>,
    Emit: Fn(Event) -> Result<(), Halted>,
    Halted: From<CheckError<Assessor::Error>>,
{
    result.input_bytes = Some(serde_json::to_vec(state).map_err(FileError::Encode)?.len());
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
    let key = Answers::derive_profile_key(context.evidence.plan.context_profile, &questions, state);
    result.evidence_hash = Some(key.clone());
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
        result.requests = 1;
        (context.emit)(Event::StageStarted {
            contract: context.contract.name.clone(),
            path: result.path.clone(),
            stage: Stage::Applicability,
        })
        .map_err(Halt::Check)?;
        let batch = match context.evaluator.assess_questions(&questions, state).await {
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
                            source: Box::new(error),
                        }
                        .into(),
                    ));
                }
                return Err(FileError::Request(error).into());
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
    apply_answers(
        context.min_confidence,
        context.emit,
        applicability,
        verification,
        result,
    )
    .map_err(Halt::Check)
}

pub(super) fn apply_answers<Emit, Halted>(
    min_confidence: f64,
    emit: &Emit,
    applicability: ev_grep_core::Assessment,
    verification: ev_grep_core::Assessment,
    result: &mut CheckResult,
) -> Result<(), Halted>
where
    Emit: Fn(Event) -> Result<(), Halted>,
{
    let applicability_routing = applicability.route(min_confidence);
    let verification_routing = verification.route(min_confidence);
    for (stage, routing) in [
        (Stage::Applicability, &applicability_routing),
        (Stage::Verification, &verification_routing),
    ] {
        emit(Event::StageCompleted {
            contract: result.contract.clone(),
            path: result.path.clone(),
            stage,
            outcome: routing.outcome,
        })?;
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
