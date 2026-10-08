use crate::{CheckResult, Event, Plan, Stage, Status};
use anyhow::{Context as _, Result};
use ev_grep_core::{Evaluator, Outcome};
use std::{cell::Cell, path::Path, time::Instant};
use tenet_contracts::Contract;

pub(super) struct Context<'a, E, F> {
    pub(super) plan: &'a Plan,
    pub(super) contract: &'a Contract,
    pub(super) evaluator: &'a E,
    pub(super) emit: &'a F,
    pub(super) requests: &'a Cell<usize>,
    pub(super) max_requests: usize,
    pub(super) min_confidence: f64,
    pub(super) local_requests: &'a Cell<usize>,
}

pub(super) async fn assess<E: Evaluator, F: Fn(Event) -> Result<()>>(
    context: &Context<'_, E, F>,
    path: &Path,
) -> Result<CheckResult> {
    let started = Instant::now();
    let contract = context.contract;
    let mode = context.plan.mode;
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
        source_hash: None,
        input_bytes: None,
        status: Status::Error,
        message: contract.message.clone(),
        reason: None,
        applicability: None,
        applicability_routing: None,
        verification: None,
        verification_routing: None,
        request: None,
        elapsed_ms: 0,
    };
    (context.emit)(Event::CheckStarted {
        contract: contract.name.clone(),
        path: result.path.clone(),
    })?;
    let change = context.plan.change(path);
    let judged = async {
        let change = change?;
        result.change_kind = Some(change.kind);
        result.previous_path = change
            .previous_path
            .as_ref()
            .map(|p| p.display().to_string());
        result.before_hash = change
            .before
            .as_ref()
            .map(|s| blake3::hash(s.as_bytes()).to_hex().to_string());
        result.after_hash = change
            .after
            .as_ref()
            .map(|s| blake3::hash(s.as_bytes()).to_hex().to_string());
        let source = change.source()?;
        result.source_hash = Some(blake3::hash(source.text.as_bytes()).to_hex().to_string());
        judge(context, &change, &mut result).await
    }
    .await;
    if let Err(error) = judged {
        result.status = Status::Error;
        result.reason = Some(error.to_string());
    }
    result.elapsed_ms = started.elapsed().as_millis();
    Ok(result)
}

async fn judge<E: Evaluator, F: Fn(Event) -> Result<()>>(
    context: &Context<'_, E, F>,
    change: &crate::Change,
    result: &mut CheckResult,
) -> Result<()> {
    let state = context.plan.file_input(context.contract, change)?;
    result.input_bytes = Some(serde_json::to_vec(&state)?.len());
    if context.requests.get() >= context.max_requests {
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
    })?;
    let relevance = super::prompt::query(Stage::Applicability, result.mode);
    let violation = super::prompt::query(Stage::Verification, result.mode);
    let mut batch = context
        .evaluator
        .assess_questions(
            &[
                ev_grep_core::Question {
                    id: "relevance",
                    query: &relevance,
                },
                ev_grep_core::Question {
                    id: "violation",
                    query: &violation,
                },
            ],
            &state,
        )
        .await?;
    result.request = Some(batch.request);
    let applicability = batch
        .answers
        .remove("relevance")
        .context("missing relevance answer")?;
    let verification = batch
        .answers
        .remove("violation")
        .context("missing violation answer")?;
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
        })?;
    }
    result.status = if applicability_routing.outcome == Outcome::NoMatch {
        Status::NotApplicable
    } else {
        match verification_routing.outcome {
            Outcome::Match => Status::Fail,
            Outcome::NoMatch => Status::Pass,
            Outcome::Uncertain => Status::Uncertain,
        }
    };
    result.applicability = Some(applicability);
    result.applicability_routing = Some(applicability_routing);
    result.verification = Some(verification);
    result.verification_routing = Some(verification_routing);
    Ok(())
}
