use std::{
    cell::RefCell,
    io::{self, Write},
    path::{Component, Path},
    time::Instant,
};

use anyhow::{Context, Result, ensure};
use ev_grep_jev::Jev;
use tenet_engine::{
    Coverage, Event, Exclusion, Mode, Options, PreparedSnapshot, Review, RunInfo, Selection,
    Status,
    review::{CheckRequest, Checked},
};

use crate::{
    cli::{Check, Cli, Command, Contracts, Input, ReporterKind, Run},
    provider::Model,
    reporter::Reporter,
};

pub(crate) async fn execute(cli: &Cli) -> Result<u8> {
    match &cli.command {
        Command::Validate => {
            let contracts = tenet_engine::discover_contracts(&cli.root)?;
            ensure!(!contracts.is_empty(), "no contracts found");
            let installed = tenet_engine::adoption::validate_lock(&cli.root)?;
            writeln!(io::stdout(), "{} contracts valid", contracts.len())?;
            if installed > 0 {
                writeln!(io::stdout(), "{installed} installed contracts recorded")?;
            }
            Ok(0)
        }
        Command::Contracts { command } => {
            match command {
                Contracts::Adoption(command) => {
                    return crate::adoption::execute(&cli.root, command);
                }
                Contracts::List { path, base, json } => {
                    if let Some(path) = path {
                        ensure!(
                            path.components()
                                .all(|c| matches!(c, Component::Normal(_) | Component::CurDir)),
                            "--for path must be relative to the project root"
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
                        .filter(|c| path.as_ref().is_none_or(|path| c.governs(path)))
                        .collect();
                    if *json {
                        let entries: Vec<_> = selected.iter().map(|c| serde_json::json!({"name":c.name,"path":c.path,"scope":c.scope,"message":c.message})).collect();
                        writeln!(
                            io::stdout(),
                            "{}",
                            serde_json::json!({"version":1,"contracts":entries,"contract_changes":contract_changes,"coverage":coverage})
                        )?;
                    } else {
                        for c in selected {
                            writeln!(io::stdout(), "{}  {}", c.name, c.path.display())?;
                        }
                    }
                }
                Contracts::Show { name } => {
                    let contracts = tenet_engine::discover_contracts(&cli.root)?;
                    let c = contracts
                        .iter()
                        .find(|c| &c.name == name)
                        .context("unknown contract")?;
                    writeln!(io::stdout(), "{}\n\n{}", c.path.display(), c.document)?;
                }
            }
            Ok(0)
        }
        Command::Check(check) => evaluate(&cli.root, &cli.state(), check).await,
        Command::Status { base, json } => {
            crate::review::status(&cli.root, &cli.state(), base.as_deref(), *json).await
        }
        Command::Show {
            contract: Some(contract),
            paths,
            json,
            ..
        } => crate::review::show(&cli.root, &cli.state(), contract, paths, *json).await,
        Command::Show {
            contract: None,
            json,
            ..
        } => crate::review::show_pending(&cli.root, &cli.state(), *json).await,
        Command::Record(record) => crate::review::record(&cli.root, &cli.state(), record).await,
        Command::Log { contract, json } => {
            crate::review::log(&cli.state(), contract.as_deref(), *json)
        }
        Command::Export(export) => {
            let (plan, _selection, _snapshot) = prepare(&cli.root, &export.input)?;
            let capture = plan.export_evidence(export.max_bytes as usize);
            let bytes = tokio::select! {
                result = capture => result?,
                signal = tokio::signal::ctrl_c() => {
                    signal?;
                    return Ok(130);
                }
            };
            io::stdout().write_all(&bytes)?;
            Ok(0)
        }
    }
}

fn prepare(
    root: &Path,
    input: &Input,
) -> Result<(tenet_engine::Plan, Selection, Option<PreparedSnapshot>)> {
    let snapshot = input
        .snapshot
        .as_ref()
        .map(|base| PreparedSnapshot::new(base, input.patch.as_deref()))
        .transpose()?;
    let root = snapshot.as_ref().map_or(root, PreparedSnapshot::root);
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
                "discovery found no files there; ignore files and protected names such as .git and .env exclude files"
            }
            Exclusion::NotGoverned => "no selected contract governs its files",
        };
        writeln!(
            io::stderr(),
            "warning: {} is not checked: {reason}",
            excluded.path.display()
        )?;
    }
    Ok((plan, selection, snapshot))
}

async fn evaluate(root: &Path, state: &Path, check: &Check) -> Result<u8> {
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
    let reporter = RefCell::new(Reporter::new(kind, &plan.contracts));
    if check.dry_run {
        let evidence = plan.capture().await?;
        // Answers a check would reuse; reading the review writes nothing.
        let mut review = if snapshot.is_some() {
            Review::ephemeral()
        } else {
            Review::read(state)?
        };
        review.use_model(&identity(run)?, !check.fresh)?;
        let prediction = tenet_engine::predict(&evidence, &review, run.jobs.into()).await?;
        let mut errors = 0;
        // Contracts whose combined input stays over the limit after reduction.
        let mut over_limit = 0;
        for c in &plan.contracts {
            let scope = (kind == ReporterKind::Jsonl).then(|| plan.requested_scope(c));
            for path in plan.files.iter().filter(|path| plan.governs(c, path)) {
                let previous_path = plan.previous_paths.get(path);
                let input = evidence.file_input(c, path);
                let (input_bytes, omitted, note) = match input {
                    Ok((state, omitted)) => {
                        (Some(serde_json::to_vec(&state)?.len()), omitted, None)
                    }
                    Err(error) => {
                        let (status, reason) = tenet_engine::input_status(&error);
                        errors += usize::from(status == Status::Error);
                        (None, Vec::new(), Some((status, reason)))
                    }
                };
                if kind == ReporterKind::Jsonl {
                    let (status, reason) = note.unzip();
                    writeln!(
                        io::stdout(),
                        "{}",
                        serde_json::json!({"version":4,"type":"selected","contract":c.name,"scope":scope,"path":path,"previous_path":previous_path,"contract_hash":c.hash,"input_bytes":input_bytes,"input_limit_bytes":tenet_engine::MAX_FILE_BYTES,"omitted":omitted,"status":status,"reason":reason})
                    )?;
                } else {
                    let note = match (&note, omitted.as_slice()) {
                        (Some((status, reason)), _) => format!(
                            "  {}: {reason}",
                            crate::report_format::status(*status).to_lowercase()
                        ),
                        (None, []) => String::new(),
                        (None, omitted) => format!("  {}", crate::report_format::omitted(omitted)),
                    };
                    let renamed = previous_path
                        .map(|old| format!(" (renamed from {})", old.display()))
                        .unwrap_or_default();
                    writeln!(
                        io::stdout(),
                        "{}  {}{renamed}{note}",
                        c.name,
                        path.display()
                    )?;
                }
            }
            let context = evidence.combined_summary(c);
            if let Ok(context) = &context
                && !plan.untouched(c)
                && !context.fits()
            {
                over_limit += 1;
            }
            if kind == ReporterKind::Jsonl {
                let (context, error) = match context {
                    Ok(context) => (Some(context), None),
                    Err(error) => (None, Some(format!("{error:#}"))),
                };
                writeln!(
                    io::stdout(),
                    "{}",
                    serde_json::json!({"version":4,"type":"combined","contract":c.name,"context":context,"error":error})
                )?;
            } else {
                writeln!(
                    io::stdout(),
                    "{}  combined {}",
                    c.name,
                    match context {
                        Ok(context) => crate::report_format::context(&context),
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
            .and_then(crate::report_format::coverage);
        if kind == ReporterKind::Jsonl {
            writeln!(
                io::stdout(),
                "{}",
                serde_json::json!({"version":4,"type":"summary","dry_run":true,"files":plan.files.len(),"mode":plan.mode,"assume_base_valid":plan.mode == Mode::Diff,"deleted":plan.deleted,"requests":unsent + prediction.cached,"cached":prediction.cached,"max_requests":budget,"combined_over_limit":over_limit,"coverage":plan.coverage,"errors":errors,"exit_code":if errors > 0 { 2 } else { 0 }})
            )?;
        } else if plan.contracts.is_empty() {
            // A full check with nothing to assess has already warned about its selection.
            if let Some(base) = &selection.base {
                match (coverage, plan.candidate_files.is_empty()) {
                    (Some(coverage), _) => writeln!(io::stdout(), "{coverage}; nothing to check.")?,
                    (None, true) => {
                        writeln!(io::stdout(), "No changes since {base}; nothing to check.")?
                    }
                    (None, false) => writeln!(
                        io::stdout(),
                        "No contract governs the changes since {base}; nothing to check."
                    )?,
                }
            }
        } else {
            writeln!(
                io::stdout(),
                "Requests  {unsent} to send ({} file, {} combined), {} answered from earlier checks; budget {budget}\nCombined  {over_limit} of {} contracts over the input limit after reduction",
                prediction.file_requests,
                prediction.combined_requests,
                prediction.cached,
                plan.contracts.len()
            )?;
            if let Some(coverage) = coverage {
                writeln!(io::stdout(), "{coverage}")?;
            }
        }
        if unsent > budget {
            writeln!(
                io::stderr(),
                "warning: {unsent} predicted requests exceed --max-requests {budget}; {} checks would stay incomplete",
                unsent - budget
            )?;
        }
        return Ok(if errors > 0 { 2 } else { 0 });
    }
    // A snapshot is a temporary copy, so its review is discarded with it.
    let mut review = if snapshot.is_some() {
        Review::ephemeral()
    } else {
        Review::open(state)?
    };
    let evidence = plan.capture().await?;
    let emit = |event| reporter.borrow_mut().emit(event);
    let begin = |provider: Option<String>, model: Option<String>| RunInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        root: plan.root.display().to_string(),
        provider,
        model,
        mode: plan.mode.as_str().into(),
        base: plan.base.clone(),
        head: plan.head.clone(),
        assumption: (plan.mode == Mode::Diff)
            .then(|| "The base satisfies the selected contracts.".into()),
        snapshot: snapshot.map(|s| s.snapshot.display().to_string()),
        patch_hash: snapshot.and_then(|s| s.patch_hash.clone()),
        jobs: run.jobs.into(),
        max_requests: run.max_requests as usize,
        min_confidence: run.min_confidence,
        deleted_contracts: plan.deleted_contracts(),
    };
    if check.no_model {
        emit(Event::Begin {
            run: begin(None, None),
        })?;
        let checked = tenet_engine::carry(&evidence, &review, &emit)?;
        let request = CheckRequest {
            selection: &selection,
            context: &check.input.context,
            model: None,
            min_confidence: None,
        };
        let revision = review.commit(&evidence, &request, &checked)?;
        return finish(&emit, checked, revision, plan.coverage.clone(), started);
    }
    let model = run.provider.model(run.model.as_deref())?;
    let identity = identity(run)?;
    // A diff that no contract governs sends no request, so it needs no credential.
    let evaluator = if plan.contracts.iter().all(|c| plan.untouched(c)) {
        Model::Unneeded
    } else {
        let key = std::env::var(run.provider.key_variable()).with_context(|| {
            format!(
                "set {} to run model evaluations",
                run.provider.key_variable()
            )
        })?;
        Model::Jev(Box::new(
            Jev::with_endpoint(
                run.provider,
                &model,
                &key,
                run.endpoint.as_deref().unwrap_or(run.provider.endpoint()),
            )?
            .with_jobs(usize::from(run.jobs))?,
        ))
    };
    review.use_model(&identity, !check.fresh)?;
    emit(Event::Begin {
        run: begin(Some(run.provider.to_string()), Some(model)),
    })?;
    let options = Options {
        jobs: run.jobs.into(),
        max_requests: run.max_requests as usize,
        min_confidence: run.min_confidence,
        classify: crate::provider::classify,
    };
    let mut refresh = tokio::time::interval(std::time::Duration::from_millis(100));
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let live = reporter.borrow().is_live();
    let checked = {
        let future = tenet_engine::run(&evidence, options, &evaluator, &review, &emit);
        tokio::pin!(future);
        loop {
            tokio::select! {
                result = &mut future => break result?,
                _ = refresh.tick(), if live => reporter.borrow_mut().refresh()?,
                signal = tokio::signal::ctrl_c() => {
                    signal?;
                    // Answers already received stay in the cache; an incomplete check adds no
                    // revision.
                    let mut summary = reporter.borrow().summary.clone();
                    summary.cancelled = true;
                    summary.elapsed_ms = started.elapsed().as_millis();
                    emit(Event::Summary {
                        summary,
                        revision: None,
                        pending: Vec::new(),
                        coverage: plan.coverage.clone(),
                        exit_code: 130,
                    })?;
                    return Ok(130);
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
    finish(&emit, checked, revision, plan.coverage.clone(), started)
}

/// The provider and model whose answers the review caches and reuses.
fn identity(run: &Run) -> Result<String> {
    let model = run.provider.model(run.model.as_deref())?;
    Ok(format!("{}/{model}", run.provider))
}

fn finish(
    emit: &impl Fn(Event) -> Result<()>,
    checked: Checked,
    revision: Option<u64>,
    coverage: Option<Coverage>,
    started: Instant,
) -> Result<u8> {
    let code = checked.exit_code();
    let pending = checked.pending();
    let mut summary = checked.summary;
    summary.elapsed_ms = started.elapsed().as_millis();
    emit(Event::Summary {
        summary,
        revision,
        pending,
        coverage,
        exit_code: code,
    })?;
    Ok(code)
}
