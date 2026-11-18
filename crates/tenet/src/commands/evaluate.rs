use core::{cell::RefCell, time::Duration};
use std::{env, path::Path, time::Instant};

use anyhow::Context as _;
use ev_grep_jev::Jev;
use tenet_engine::{Event, Mode, Options, Review, RunInfo, review::CheckRequest};
use tokio::{signal, time, time::MissedTickBehavior};

use super::{finish, prepare, report_dry_run};
use crate::{
    cli::{Check, ExitStatus, ReporterKind},
    error::CommandError,
    provider::Model,
    reporter::Reporter,
};

#[expect(
    clippy::future_not_send,
    reason = "The command drives this future on one current-thread runtime and never moves it \
              across threads."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
#[expect(
    clippy::integer_division_remainder_used,
    reason = "tokio::select! expands to a remainder for its branch order."
)]
pub(super) async fn evaluate(
    root: &Path,
    state: &Path,
    check: &Check,
) -> Result<ExitStatus, CommandError> {
    if let Some(name) = &check.local {
        return super::local::run(root, state, name, check.fresh, check.run.json).await;
    }
    // The summary's duration covers the whole check, from discovery to the review update.
    let started = Instant::now();
    let (plan, selection, prepared) = prepare(root, &check.input)?;
    let snapshot = prepared.as_ref();
    let run = &check.run;
    let budget = run.request_budget().context("convert request budget")?;
    let kind = if run.json {
        ReporterKind::Jsonl
    } else {
        run.reporter
    };
    let reporter = RefCell::new(Reporter::new(kind, &plan.contracts, state));
    if check.dry_run {
        return report_dry_run(&plan, &selection, snapshot, state, check, kind).await;
    }

    let evidence = plan.capture().await?;
    match evidence.admit_context() {
        Err(error) if error.is_size_limit() => {},
        result => result?,
    }
    // A snapshot is a temporary copy, so its review is discarded with it.
    let mut review = if snapshot.is_some() {
        Review::new_ephemeral()
    } else {
        Review::open(state)?
    };
    let emit = |event| reporter.borrow_mut().emit(event);
    let begin = |provider: Option<String>, model: Option<String>| {
        RunInfo {
            version: env!("CARGO_PKG_VERSION").into(),
            root: plan.root.display().to_string(),
            provider,
            model,
            mode: plan.mode.as_str().into(),
            base: plan.base.clone(),
            head: plan.head.clone(),
            assumption: (plan.mode == Mode::Diff)
                .then(|| "The base satisfies the selected contracts.".into()),
            snapshot: snapshot.map(|saved| saved.snapshot.display().to_string()),
            patch_hash: snapshot.and_then(|saved| saved.patch_hash.clone()),
            jobs: run.jobs.into(),
            contract_batch_size: usize::from(run.contract_batch_size),
            context_profile: plan.context_profile,
            input_limit_bytes: plan.read_input_limit_bytes(),
            max_requests: budget,
            min_confidence: run.min_confidence,
            deleted_contracts: plan.list_deleted_contracts(),
        }
    };
    if check.no_model {
        emit(Event::Begin {
            run: begin(None, None),
        })?;
        let checked = tenet_engine::apply_review(&evidence, &review, &emit)?;
        let request = CheckRequest {
            context: &check.input.context,
            contract_batch_size: usize::from(run.contract_batch_size),
            min_confidence: None,
            model: None,
            selection: &selection,
        };
        let revision = review.commit(&evidence, &request, &checked)?;
        return finish(&emit, &checked, revision, plan.coverage.clone(), started);
    }
    let model = run.provider.select_model(run.model.as_deref())?;
    // The provider and model the revision names.
    let identity = format!("{}/{model}", run.provider);
    // A diff that no contract governs sends no request, so it needs no credential.
    let evaluator = if plan
        .contracts
        .iter()
        .all(|contract| plan.is_untouched(contract))
    {
        Model::Unneeded
    } else {
        let key = env::var(run.provider.read_key_variable()).with_context(|| {
            format!(
                "set {:?} to run model evaluations",
                run.provider.read_key_variable()
            )
        })?;
        Model::Jev(Box::new(
            Jev::with_endpoint(
                run.provider,
                &model,
                &key,
                run.endpoint
                    .as_deref()
                    .unwrap_or_else(|| run.provider.read_endpoint()),
            )?
            .with_jobs(usize::from(run.jobs))?
            .with_context_profile(check.input.context_profile.read_profile()),
        ))
    };
    review.use_model(&run.provider.to_string(), &model, !check.fresh)?;
    emit(Event::Begin {
        run: begin(Some(run.provider.to_string()), Some(model)),
    })?;
    let options = Options {
        contract_batch_size: usize::from(run.contract_batch_size),
        jobs: run.jobs.into(),
        max_requests: budget,
        min_confidence: run.min_confidence,
    };
    let mut refresh = time::interval(Duration::from_millis(100));
    refresh.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let live = reporter.borrow().is_live();
    let checked = {
        let future = tenet_engine::run(&evidence, options, &evaluator, &review, &emit);
        tokio::pin!(future);
        loop {
            tokio::select! {
                result = &mut future => break result?,
                _ = refresh.tick(), if live => reporter.borrow_mut().refresh()?,
                signal = signal::ctrl_c() => {
                    signal?;
                    // Answers already received stay in the cache; an incomplete check adds no
                    // revision.
                    let mut summary = reporter.borrow().summary;
                    summary.cancelled = true;
                    summary.elapsed_ms = started.elapsed().as_millis();
                    emit(Event::Summary {
                        summary,
                        revision: None,
                        pending: Vec::new(),
                        coverage: plan.coverage.clone(),
                        exit_code: u8::from(ExitStatus::Cancelled),
                    })?;
                    return Ok(ExitStatus::Cancelled);
                }
            }
        }
    };
    let request = CheckRequest {
        context: &check.input.context,
        contract_batch_size: usize::from(run.contract_batch_size),
        min_confidence: Some(run.min_confidence),
        model: Some(&identity),
        selection: &selection,
    };
    let revision = review.commit(&evidence, &request, &checked)?;
    finish(&emit, &checked, revision, plan.coverage.clone(), started)
}
