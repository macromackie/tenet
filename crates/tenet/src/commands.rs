use core::{cell::RefCell, time::Duration};
use std::{
    env,
    io::Write as _,
    path::{Component, Path},
    time::Instant,
};

use anyhow::{Context as _, anyhow};
use ev_grep_jev::Jev;
use tenet_engine::{
    Coverage,
    Event,
    Exclusion,
    Mode,
    Options,
    PreparedSnapshot,
    Review,
    RunInfo,
    Selection,
    Status,
    adoption::{self, AddRequest, Destination, Kind, SkillDestination, SkillRequest},
    review::{CheckRequest, Checked},
};
use tokio::{signal, time, time::MissedTickBehavior};

use crate::{
    adoption::{execute as execute_adoption, report_added},
    cli::{Check, Cli, Command, Contracts, ExitStatus, Input, ReporterKind, Skills},
    error::CommandError,
    output,
    provider::Model,
    report_format,
    reporter::Reporter,
    review,
};

/// The schema version of the object that `contracts list --json` prints.
const LIST_VERSION: u64 = 1;

#[expect(
    clippy::as_conversions,
    reason = "The converted values are request budgets, byte caps, durations and fixture \
              positions that fit the target type."
)]
#[expect(
    clippy::cognitive_complexity,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
#[expect(
    clippy::excessive_nesting,
    reason = "Each level is a loop, match or closure over one parsed structure, and flattening it \
              would split one decision."
)]
#[expect(
    clippy::future_not_send,
    reason = "The command drives this future on one current-thread runtime and never moves it \
              across threads."
)]
#[expect(
    clippy::integer_division_remainder_used,
    reason = "The quotient and remainder pick a spinner frame or a whole time unit, and \
              tokio::select! expands to a remainder for its branch order."
)]
#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
pub(crate) async fn execute(cli: &Cli) -> Result<ExitStatus, CommandError> {
    match &cli.command {
        Command::Validate => {
            let contracts = tenet_engine::discover_contracts(&cli.root)?;
            let recorded = adoption::validate_lock(&cli.root)?;
            // A project may install skills without having contracts of its own.
            if contracts.is_empty() && recorded.skills == 0 {
                return Err(anyhow!("no contracts found").into());
            }
            if !contracts.is_empty() {
                writeln!(output::open_stdout(), "{} contracts valid", contracts.len())?;
            }
            if recorded.contracts > 0 {
                writeln!(
                    output::open_stdout(),
                    "{} installed contracts recorded",
                    recorded.contracts
                )?;
            }
            if recorded.skills > 0 {
                writeln!(
                    output::open_stdout(),
                    "{} installed skills recorded",
                    recorded.skills
                )?;
            }
            Ok(ExitStatus::Passed)
        },
        Command::Skills { command } => {
            match command {
                Skills::Add {
                    spec,
                    dir,
                    name,
                    path,
                    sources,
                } => {
                    let destination = match path {
                        Some(path) => SkillDestination::Existing(path),
                        None => {
                            SkillDestination::Directory {
                                directory: dir,
                                name: name.as_deref(),
                            }
                        },
                    };
                    let added = adoption::add_skill(
                        &cli.root,
                        &SkillRequest {
                            spec,
                            destination,
                            sources,
                        },
                    )?;
                    report_added(&added, "tenet validate")
                },
                Skills::Locked(command) => execute_adoption(&cli.root, Kind::Skills, command),
            }
        },
        Command::Contracts { command } => {
            match command {
                Contracts::Add {
                    spec,
                    scope,
                    path,
                    name,
                    number,
                    sources,
                } => {
                    let destination = match (scope, path) {
                        (_, Some(path)) => Destination::Existing(path),
                        (Some(scope), None) => {
                            Destination::Scope {
                                scope,
                                name: name.as_deref(),
                                number: *number,
                            }
                        },
                        (None, None) => {
                            return Err(anyhow!("add requires --scope or --path").into());
                        },
                    };
                    let added = adoption::add(
                        &cli.root,
                        &AddRequest {
                            spec,
                            destination,
                            sources,
                        },
                    )?;
                    return report_added(&added, &format!("tenet check --contract {}", added.name));
                },
                Contracts::Locked(command) => {
                    return execute_adoption(&cli.root, Kind::Contracts, command);
                },
                Contracts::List { path, base, json } => {
                    if let Some(path) = path
                        && !path.components().all(|component| {
                            match component {
                                Component::Normal(_) | Component::CurDir => true,
                                Component::Prefix(_)
                                | Component::RootDir
                                | Component::ParentDir => false,
                            }
                        })
                    {
                        return Err(
                            anyhow!("--for path must be relative to the project root").into()
                        );
                    }
                    let (contracts, contract_changes, coverage) = if let Some(base) = base {
                        let plan = tenet_engine::plan(
                            &cli.root,
                            &Selection {
                                base: Some(base.clone()),
                                ..Selection::default()
                            },
                        )?;
                        (plan.contracts, plan.contract_changes, plan.coverage)
                    } else {
                        (
                            tenet_engine::discover_contracts(&cli.root)?,
                            Vec::new(),
                            None,
                        )
                    };
                    let selected: Vec<_> = contracts
                        .iter()
                        .filter(|contract| {
                            path.as_ref().is_none_or(|path| contract.is_governing(path))
                        })
                        .collect();
                    if *json {
                        let entries: Vec<_> = selected
                            .iter()
                            .map(|contract| {
                                serde_json::json!({
                                    "name": contract.name,
                                    "path": contract.path,
                                    "scope": contract.scope,
                                    "message": contract.message,
                                })
                            })
                            .collect();
                        output::write_document(
                            &mut output::open_stdout(),
                            LIST_VERSION,
                            serde_json::json!({
                                "contracts": entries,
                                "contract_changes": contract_changes,
                                "coverage": coverage,
                            }),
                        )?;
                    } else {
                        for contract in selected {
                            writeln!(
                                output::open_stdout(),
                                "{}  {}",
                                contract.name,
                                contract.path.display()
                            )?;
                        }
                    }
                },
                Contracts::Show { name } => {
                    let contracts = tenet_engine::discover_contracts(&cli.root)?;
                    let contract = contracts
                        .iter()
                        .find(|contract| &contract.name == name)
                        .context("unknown contract")?;
                    writeln!(
                        output::open_stdout(),
                        "{}\n\n{}",
                        contract.path.display(),
                        contract.document
                    )?;
                },
            }
            Ok(ExitStatus::Passed)
        },
        Command::Check(check) => evaluate(&cli.root, &cli.resolve_state(), check).await,
        Command::Status {
            base,
            json,
            prune_cache,
        } => {
            review::report_status(
                &cli.root,
                &cli.resolve_state(),
                base.as_deref(),
                *json,
                *prune_cache,
            )
            .await
        },
        Command::Show {
            contract: Some(contract),
            paths,
            json,
            ..
        } => review::show(&cli.root, &cli.resolve_state(), contract, paths, *json).await,
        Command::Show {
            contract: None,
            json,
            ..
        } => review::show_pending(&cli.root, &cli.resolve_state(), *json).await,
        Command::Record(record) => review::record(&cli.root, &cli.resolve_state(), record).await,
        Command::Log { contract, json } => {
            review::log(&cli.resolve_state(), contract.as_deref(), *json)
        },
        Command::Export(export) => {
            let (plan, _selection, _snapshot) = prepare(&cli.root, &export.input)?;
            let capture = plan.export_evidence(export.max_bytes as usize);
            let bytes = tokio::select! {
                result = capture => result?,
                signal = signal::ctrl_c() => {
                    signal?;
                    return Ok(ExitStatus::Cancelled);
                }
            };
            output::open_stdout().write_all(&bytes)?;
            Ok(ExitStatus::Passed)
        },
    }
}

#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
fn prepare(
    root: &Path,
    input: &Input,
) -> Result<(tenet_engine::Plan, Selection, Option<PreparedSnapshot>), CommandError> {
    let snapshot = input
        .snapshot
        .as_ref()
        .map(|base| PreparedSnapshot::new(base, input.patch.as_deref()))
        .transpose()?;
    let root = snapshot.as_ref().map_or(root, PreparedSnapshot::read_root);
    let selection = Selection {
        paths: input.paths.clone(),
        base: if input.patch.is_some() {
            Some("HEAD".into())
        } else {
            input.base.clone()
        },
        contract: input.contract.clone(),
    };
    let plan = tenet_engine::plan(root, &selection)?.with_context(&input.context)?;
    plan.admit_context()?;
    for excluded in &plan.excluded_paths {
        let reason = match excluded.reason {
            Exclusion::NotDiscovered => {
                "discovery found no files there; ignore files and protected names such as .git and \
                 .env exclude files"
            },
            Exclusion::NotGoverned => "no selected contract governs its files",
        };
        writeln!(
            output::open_stderr(),
            "warning: {} is not checked: {reason}",
            excluded.path.display()
        )?;
    }
    Ok((plan, selection, snapshot))
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "Counts are bounded by the repository's contracts and the request budget, and the \
              subtraction runs only when the requests exceed the budget."
)]
#[expect(
    clippy::as_conversions,
    reason = "The converted values are request budgets, byte caps, durations and fixture \
              positions that fit the target type."
)]
#[expect(
    clippy::cognitive_complexity,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
#[expect(
    clippy::excessive_nesting,
    reason = "Each level is a loop, match or closure over one parsed structure, and flattening it \
              would split one decision."
)]
#[expect(
    clippy::future_not_send,
    reason = "The command drives this future on one current-thread runtime and never moves it \
              across threads."
)]
#[expect(
    clippy::integer_division_remainder_used,
    reason = "The quotient and remainder pick a spinner frame or a whole time unit, and \
              tokio::select! expands to a remainder for its branch order."
)]
#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
#[expect(
    clippy::shadow_unrelated,
    reason = "Each block binds its own result under the name the following lines read."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
async fn evaluate(root: &Path, state: &Path, check: &Check) -> Result<ExitStatus, CommandError> {
    // The summary's duration covers the whole check, from discovery to the review update.
    let started = Instant::now();
    let (plan, selection, prepared) = prepare(root, &check.input)?;
    let snapshot = prepared.as_ref();
    let run = &check.run;
    let kind = if run.json {
        ReporterKind::Jsonl
    } else {
        run.reporter
    };
    let reporter = RefCell::new(Reporter::new(kind, &plan.contracts, state));
    if check.dry_run {
        let evidence = plan.capture().await?;
        // Answers a check would reuse; reading the review writes nothing.
        let mut review = if snapshot.is_some() {
            Review::new_ephemeral()
        } else {
            Review::read(state)?
        };
        review.use_model(
            &run.provider.to_string(),
            &run.provider.select_model(run.model.as_deref())?,
            !check.fresh,
        )?;
        let prediction = tenet_engine::estimate(&evidence, &review, run.jobs.into()).await?;
        let mut errors = 0;
        // Contracts whose combined input stays over the limit after reduction.
        let mut over_limit = 0_i32;
        for contract in &plan.contracts {
            let scope = (kind == ReporterKind::Jsonl).then(|| plan.find_requested_scope(contract));
            for path in plan
                .files
                .iter()
                .filter(|path| plan.is_governing(contract, path))
            {
                let previous_path = plan.previous_paths.get(path);
                let input = evidence.build_file_input(contract, path);
                let (input_bytes, omitted, note) = match input {
                    Ok((state, omitted)) => {
                        (Some(serde_json::to_vec(&state)?.len()), omitted, None)
                    },
                    Err(error) => {
                        let (status, reason) = tenet_engine::classify_input_status(&error);
                        errors += usize::from(status == Status::Error);
                        (None, Vec::new(), Some((status, reason)))
                    },
                };
                if kind == ReporterKind::Jsonl {
                    let (status, reason) = note.unzip();
                    output::write_document(
                        &mut output::open_stdout(),
                        tenet_engine::EVENTS_VERSION,
                        serde_json::json!({
                            "type": "selected",
                            "contract": contract.name,
                            "scope": scope,
                            "path": path,
                            "previous_path": previous_path,
                            "contract_hash": contract.hash,
                            "input_bytes": input_bytes,
                            "input_limit_bytes": tenet_engine::MAX_FILE_BYTES,
                            "omitted": omitted,
                            "status": status,
                            "reason": reason,
                        }),
                    )?;
                } else {
                    let note = match (&note, omitted.as_slice()) {
                        (Some((status, reason)), _) => {
                            format!(
                                "  {}: {reason}",
                                report_format::label_status(*status).to_lowercase()
                            )
                        },
                        (None, []) => String::new(),
                        (None, omitted) => format!("  {}", report_format::format_omitted(omitted)),
                    };
                    let renamed = previous_path
                        .map(|old| format!(" (renamed from {})", old.display()))
                        .unwrap_or_default();
                    writeln!(
                        output::open_stdout(),
                        "{}  {}{renamed}{note}",
                        contract.name,
                        path.display()
                    )?;
                }
            }
            let context = evidence.summarize_combined(contract);
            if let Ok(context) = &context
                && !plan.is_untouched(contract)
                && !context.is_fitting()
            {
                over_limit += 1_i32;
            }
            if kind == ReporterKind::Jsonl {
                let (context, error) = match context {
                    Ok(context) => (Some(context), None),
                    Err(error) => (None, Some(format!("{error:#}"))),
                };
                output::write_document(
                    &mut output::open_stdout(),
                    tenet_engine::EVENTS_VERSION,
                    serde_json::json!({
                        "type": "combined",
                        "contract": contract.name,
                        "context": context,
                        "error": error,
                    }),
                )?;
            } else {
                writeln!(
                    output::open_stdout(),
                    "{}  combined {}",
                    contract.name,
                    match context {
                        Ok(context) => report_format::format_context(&context),
                        Err(error) => format!("unavailable: {error:#}"),
                    }
                )?;
            }
        }
        // Answers from earlier checks spend none of the budget.
        let unsent = prediction.file_requests + prediction.combined_requests;
        let budget = run.max_requests as usize;
        let coverage = plan
            .coverage
            .as_ref()
            .and_then(report_format::format_coverage);
        let status = if errors > 0 {
            ExitStatus::Error
        } else {
            ExitStatus::Passed
        };
        if kind == ReporterKind::Jsonl {
            output::write_document(
                &mut output::open_stdout(),
                tenet_engine::EVENTS_VERSION,
                serde_json::json!({
                    "type": "summary",
                    "dry_run": true,
                    "files": plan.files.len(),
                    "mode": plan.mode,
                    "assume_base_valid": plan.mode == Mode::Diff,
                    "deleted": plan.deleted,
                    "requests": unsent + prediction.cached,
                    "cached": prediction.cached,
                    "max_requests": budget,
                    "combined_over_limit": over_limit,
                    "coverage": plan.coverage,
                    "errors": errors,
                    "exit_code": u8::from(status),
                }),
            )?;
        } else if plan.contracts.is_empty() {
            // A full check with nothing to assess has already warned about its selection.
            if let Some(base) = &selection.base {
                match (coverage, plan.candidate_files.is_empty()) {
                    (Some(coverage), _) => {
                        writeln!(output::open_stdout(), "{coverage}; nothing to check.")?;
                    },
                    (None, true) => {
                        writeln!(
                            output::open_stdout(),
                            "No changes since {base}; nothing to check."
                        )?;
                    },
                    (None, false) => {
                        writeln!(
                            output::open_stdout(),
                            "No contract governs the changes since {base}; nothing to check."
                        )?;
                    },
                }
            }
        } else {
            writeln!(
                output::open_stdout(),
                "Requests  {unsent} to send ({} file, {} combined), {} answered from earlier \
                 checks; budget {budget}\nCombined  {over_limit} of {} contracts over the input \
                 limit after reduction",
                prediction.file_requests,
                prediction.combined_requests,
                prediction.cached,
                plan.contracts.len()
            )?;
            if let Some(coverage) = coverage {
                writeln!(output::open_stdout(), "{coverage}")?;
            }
        }
        if unsent > budget {
            writeln!(
                output::open_stderr(),
                "warning: {unsent} predicted requests exceed --max-requests {budget}; {} checks \
                 would stay incomplete",
                unsent - budget
            )?;
        }
        return Ok(status);
    }
    // A snapshot is a temporary copy, so its review is discarded with it.
    let mut review = if snapshot.is_some() {
        Review::new_ephemeral()
    } else {
        Review::open(state)?
    };
    let evidence = plan.capture().await?;
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
            max_requests: run.max_requests as usize,
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
            selection: &selection,
            context: &check.input.context,
            model: None,
            min_confidence: None,
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
            .with_jobs(usize::from(run.jobs))?,
        ))
    };
    review.use_model(&run.provider.to_string(), &model, !check.fresh)?;
    emit(Event::Begin {
        run: begin(Some(run.provider.to_string()), Some(model)),
    })?;
    let options = Options {
        jobs: run.jobs.into(),
        max_requests: run.max_requests as usize,
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
        selection: &selection,
        context: &check.input.context,
        model: Some(&identity),
        min_confidence: Some(run.min_confidence),
    };
    let revision = review.commit(&evidence, &request, &checked)?;
    finish(&emit, &checked, revision, plan.coverage.clone(), started)
}

fn finish(
    emit: &impl Fn(Event) -> Result<(), CommandError>,
    checked: &Checked,
    revision: Option<u64>,
    coverage: Option<Coverage>,
    started: Instant,
) -> Result<ExitStatus, CommandError> {
    let status = ExitStatus::from(checked.summarize_overall());
    let pending = checked.list_pending();
    let mut summary = checked.summary;
    summary.elapsed_ms = started.elapsed().as_millis();
    emit(Event::Summary {
        summary,
        revision,
        pending,
        coverage,
        exit_code: u8::from(status),
    })?;
    Ok(status)
}
