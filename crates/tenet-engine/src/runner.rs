use std::{cell::Cell, path::Path, time::Instant};

use anyhow::{Result, ensure};
use ev_grep_core::{Evaluator, Outcome, Source, SourceRead};
use futures_util::{StreamExt, stream};
use tenet_contracts::{Contract, Example};

use crate::{CheckResult, Event, Plan, Stage, Status, Summary};

#[derive(Clone, Copy)]
pub struct Options {
    pub jobs: usize,
    pub max_requests: usize,
}

pub async fn run(
    plan: &Plan,
    options: Options,
    evaluator: &impl Evaluator,
    emit: &impl Fn(Event) -> Result<()>,
) -> Result<Summary> {
    execute(plan, options, evaluator, emit, false).await
}

pub async fn run_examples(
    plan: &Plan,
    options: Options,
    evaluator: &impl Evaluator,
    emit: &impl Fn(Event) -> Result<()>,
) -> Result<Summary> {
    ensure!(
        plan.contracts.iter().any(|c| !c.examples.is_empty()),
        "no examples found in selected contracts"
    );
    execute(plan, options, evaluator, emit, true).await
}

async fn execute(
    plan: &Plan,
    options: Options,
    evaluator: &impl Evaluator,
    emit: &impl Fn(Event) -> Result<()>,
    evaluation: bool,
) -> Result<Summary> {
    ensure!(
        options.jobs > 0 && options.max_requests > 0,
        "jobs and max-requests must be positive"
    );
    let started = Instant::now();
    let requests = Cell::new(0);
    let mut total = Summary::default();
    for contract in &plan.contracts {
        let subjects: Vec<_> = if evaluation {
            contract.examples.iter().map(Subject::Example).collect()
        } else {
            plan.files
                .iter()
                .filter(|p| p.starts_with(&contract.scope))
                .map(|p| Subject::File(p.as_path()))
                .collect()
        };
        emit(Event::ContractStarted {
            contract: contract.name.clone(),
            path: contract.path.display().to_string(),
            total: subjects.len(),
        })?;
        let context = Context {
            root: &plan.root,
            contract,
            evaluator,
            emit,
            requests: &requests,
            max_requests: options.max_requests,
        };
        let mut running = stream::iter(subjects)
            .map(|subject| assess(&context, subject))
            .buffer_unordered(options.jobs);
        let mut summary = Summary::default();
        while let Some(result) = running.next().await {
            let result = result?;
            summary.add(&result);
            total.add(&result);
            emit(Event::Result {
                result: Box::new(result),
            })?;
        }
        emit(Event::ContractFinished {
            contract: contract.name.clone(),
            summary,
        })?;
    }
    for path in &plan.deleted {
        emit(Event::Skipped {
            path: path.display().to_string(),
            reason: "deleted path has no current contents; this is not a before/after review"
                .into(),
        })?;
    }
    total.requests = requests.get();
    total.elapsed_ms = started.elapsed().as_millis();
    Ok(total)
}

struct Context<'a, E, F> {
    root: &'a Path,
    contract: &'a Contract,
    evaluator: &'a E,
    emit: &'a F,
    requests: &'a Cell<usize>,
    max_requests: usize,
}

enum Subject<'a> {
    File(&'a Path),
    Example(&'a Example),
}

async fn assess<E: Evaluator, F: Fn(Event) -> Result<()>>(
    context: &Context<'_, E, F>,
    subject: Subject<'_>,
) -> Result<CheckResult> {
    let started = Instant::now();
    let contract = context.contract;
    let (path, example) = match subject {
        Subject::File(path) => (path.to_owned(), None),
        Subject::Example(example) => (contract.scope.join(&example.path), Some(example)),
    };
    let mut result = CheckResult {
        contract: contract.name.clone(),
        contract_path: contract.path.display().to_string(),
        contract_hash: contract.hash.clone(),
        path: path.display().to_string(),
        source_hash: None,
        example: example.map(|e| e.name.clone()),
        expected: example.map(|e| e.expected),
        status: Status::Error,
        message: contract.message.clone(),
        reason: None,
        applicability: None,
        verification: None,
        elapsed_ms: 0,
    };
    (context.emit)(Event::CheckStarted {
        contract: contract.name.clone(),
        path: result.path.clone(),
    })?;
    let source = if let Some(example) = example {
        if example.source.len() > ev_grep_core::MAX_FILE_BYTES {
            Err(anyhow::anyhow!(
                "example exceeds source limit; input was not truncated"
            ))
        } else {
            Ok(SourceRead::Text(Source {
                path: result.path.clone(),
                text: example.source.clone(),
            }))
        }
    } else {
        ev_grep_core::read_source(&context.root.join(&path))
    };
    match source {
        Ok(SourceRead::Text(mut source)) => {
            source.path.clone_from(&result.path);
            result.source_hash = Some(blake3::hash(source.text.as_bytes()).to_hex().to_string());
            if let Err(error) = judge(context, &source, &mut result).await {
                result.status = Status::Error;
                result.reason = Some(error.to_string());
            }
        }
        Ok(SourceRead::Binary) => {
            result.status = Status::NotApplicable;
            result.reason = Some("binary file; semantic text checks do not apply".into());
        }
        Err(error) => {
            result.reason = Some(error.to_string());
        }
    }
    result.elapsed_ms = started.elapsed().as_millis();
    Ok(result)
}

async fn judge<E: Evaluator, F: Fn(Event) -> Result<()>>(
    context: &Context<'_, E, F>,
    source: &Source,
    result: &mut CheckResult,
) -> Result<()> {
    for stage in [Stage::Applicability, Stage::Verification] {
        if context.requests.get() >= context.max_requests {
            result.status = Status::Incomplete;
            result.reason = Some("request budget exhausted".into());
            return Ok(());
        }
        context.requests.set(context.requests.get() + 1);
        (context.emit)(Event::StageStarted {
            contract: context.contract.name.clone(),
            path: source.path.clone(),
            stage,
        })?;
        let assessment = context
            .evaluator
            .assess(&query(context.contract, stage), source)
            .await?;
        let outcome = assessment.outcome;
        (context.emit)(Event::StageCompleted {
            contract: context.contract.name.clone(),
            path: source.path.clone(),
            stage,
            outcome,
        })?;
        match stage {
            Stage::Applicability => {
                result.applicability = Some(assessment);
                if outcome == Outcome::NoMatch {
                    result.status = Status::NotApplicable;
                    return Ok(());
                }
            }
            Stage::Verification => {
                result.status = match outcome {
                    Outcome::Match => Status::Fail,
                    Outcome::NoMatch => Status::Pass,
                    Outcome::Uncertain => Status::Uncertain,
                };
                result.reason = assessment.reason.map(|r| match r {
                    ev_grep_core::Uncertainty::InsufficientContext => {
                        "insufficient context".to_owned()
                    }
                    ev_grep_core::Uncertainty::LowConfidence => "low confidence".to_owned(),
                });
                result.verification = Some(assessment);
            }
        }
    }
    Ok(())
}

fn query(contract: &Contract, stage: Stage) -> String {
    let question = match stage {
        Stage::Applicability => {
            "Does this file contain code to which the requirement could apply? Match means relevant, not a violation. No match means clearly unrelated. Use uncertain when relevance depends on missing context."
        }
        Stage::Verification => {
            "Does the supplied file violate the requirement? Match means a violation is supported by the code. No match means the relevant behavior is visible and does not violate it, or the requirement does not apply. Use uncertain when deciding requires missing implementation or context. Respect explicit exceptions. Judge current contents, not whether a change introduced the behavior."
        }
    };
    let applies = contract
        .applies_to
        .as_deref()
        .unwrap_or("Use the requirement to determine relevance.");
    format!(
        "{question}\n\nRequirement:\n{}\n\nApplies to:\n{applies}",
        contract.rules
    )
}
