use std::{
    cell::RefCell,
    io::{self, Write},
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
use ev_grep_jev::Jev;
use tenet_engine::{
    Event, Mode, Options, PreparedSnapshot, Review, RunInfo, Selection,
    review::{CheckRequest, Checked},
};

use crate::{
    cli::{Check, Cli, Command, Contracts, Input, ReporterKind},
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
                    let (contracts, contract_changes) = if let Some(base) = base {
                        let plan = tenet_engine::plan(
                            &cli.root,
                            &Selection {
                                base: Some(base.clone()),
                                ..Selection::default()
                            },
                        )?;
                        (plan.contracts, plan.contract_changes)
                    } else {
                        (tenet_engine::discover_contracts(&cli.root)?, Vec::new())
                    };
                    let selected: Vec<_> = contracts
                        .iter()
                        .filter(|c| path.as_ref().is_none_or(|p| p.starts_with(&c.scope)))
                        .collect();
                    if *json {
                        let entries: Vec<_> = selected.iter().map(|c| serde_json::json!({"name":c.name,"path":c.path,"scope":c.scope,"message":c.message})).collect();
                        writeln!(
                            io::stdout(),
                            "{}",
                            serde_json::json!({"version":1,"contracts":entries,"contract_changes":contract_changes})
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
        Command::Check(check) => {
            let (plan, selection, snapshot) = prepare(&cli.root, &check.input)?;
            evaluate(&plan, &selection, check, snapshot.as_ref()).await
        }
        Command::Status { json } => crate::review::status(&cli.root, *json).await,
        Command::Show { contract, json } => crate::review::show(&cli.root, contract, *json).await,
        Command::Record(record) => crate::review::record(&cli.root, record).await,
        Command::Log { contract, json } => {
            crate::review::log(&cli.root, contract.as_deref(), *json)
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
    Ok((plan, selection, snapshot))
}

async fn evaluate(
    plan: &tenet_engine::Plan,
    selection: &Selection,
    check: &Check,
    snapshot: Option<&PreparedSnapshot>,
) -> Result<u8> {
    let run = &check.run;
    let kind = if run.json {
        ReporterKind::Jsonl
    } else {
        run.reporter
    };
    let reporter = RefCell::new(Reporter::new(kind, &plan.contracts));
    if check.dry_run {
        let evidence = plan.capture().await?;
        let mut errors = 0;
        for c in &plan.contracts {
            let scope = (kind == ReporterKind::Jsonl).then(|| plan.requested_scope(c));
            for path in plan.files.iter().filter(|p| {
                p.starts_with(&c.scope)
                    || plan
                        .previous_paths
                        .get(*p)
                        .is_some_and(|old| old.starts_with(&c.scope))
            }) {
                let input = evidence.file_input(c, path);
                let (input_bytes, error) = match input {
                    Ok(state) => (Some(serde_json::to_vec(&state)?.len()), None),
                    Err(error) => {
                        errors += 1;
                        (None, Some(error.to_string()))
                    }
                };
                if kind == ReporterKind::Jsonl {
                    writeln!(
                        io::stdout(),
                        "{}",
                        serde_json::json!({"version":4,"type":"selected","contract":c.name,"scope":scope,"path":path,"contract_hash":c.hash,"input_bytes":input_bytes,"input_limit_bytes":tenet_engine::MAX_FILE_BYTES,"error":error})
                    )?;
                } else {
                    writeln!(io::stdout(), "{}  {}", c.name, path.display())?;
                    if let Some(error) = error {
                        writeln!(io::stderr(), "  {error}")?;
                    }
                }
            }
        }
        if kind == ReporterKind::Jsonl {
            writeln!(
                io::stdout(),
                "{}",
                serde_json::json!({"version":4,"type":"summary","dry_run":true,"files":plan.files.len(),"mode":plan.mode,"assume_base_valid":plan.mode == Mode::Diff,"deleted":plan.deleted,"errors":errors,"exit_code":if errors > 0 { 2 } else { 0 }})
            )?;
        }
        return Ok(if errors > 0 { 2 } else { 0 });
    }
    // A snapshot is a temporary copy, so its review is discarded with it.
    let mut review = if snapshot.is_some() {
        Review::ephemeral()
    } else {
        Review::open(&plan.root)?
    };
    let evidence = plan.capture().await?;
    let emit = |event| reporter.borrow_mut().emit(event);
    let begin = |provider: Option<String>, model: Option<String>| RunInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        root: plan.root.display().to_string(),
        provider,
        model,
        mode: if plan.mode == Mode::Diff {
            "diff"
        } else {
            "full"
        }
        .into(),
        base: plan.base.clone(),
        head: plan.head.clone(),
        assumption: (plan.mode == Mode::Diff)
            .then(|| "The base satisfies the selected contracts.".into()),
        snapshot: snapshot.map(|s| s.snapshot.display().to_string()),
        patch_hash: snapshot.and_then(|s| s.patch_hash.clone()),
        jobs: run.jobs.into(),
        max_requests: run.max_requests as usize,
        min_confidence: run.min_confidence,
    };
    if check.no_model {
        emit(Event::Begin {
            run: begin(None, None),
        })?;
        let checked = review.carry(&evidence);
        for (contract, concluded) in plan.contracts.iter().zip(&checked.contracts) {
            emit(Event::ContractStarted {
                contract: concluded.contract.clone(),
                path: String::new(),
                total: plan
                    .files
                    .iter()
                    .filter(|path| plan.in_scope(path, &contract.scope))
                    .count(),
            })?;
            emit(Event::ContractFinished {
                contract: concluded.contract.clone(),
                summary: tenet_engine::Summary::default(),
                conclusion: None,
                state: concluded.state.clone(),
            })?;
        }
        let request = CheckRequest {
            selection,
            context: &check.input.context,
            model: None,
        };
        let revision = review.commit(&evidence, &request, &checked)?;
        return finish(&emit, checked, revision);
    }
    let model = run.provider.model(run.model.as_deref())?;
    let key = std::env::var(run.provider.key_variable()).with_context(|| {
        format!(
            "set {} to run model evaluations",
            run.provider.key_variable()
        )
    })?;
    let evaluator = Jev::with_endpoint(
        run.provider,
        &model,
        &key,
        run.endpoint.as_deref().unwrap_or(run.provider.endpoint()),
    )?
    .with_jobs(usize::from(run.jobs))?;
    let identity = format!("{}/{model}", run.provider);
    review.use_model(&identity, !check.fresh)?;
    emit(Event::Begin {
        run: begin(Some(run.provider.to_string()), Some(model)),
    })?;
    let options = Options {
        jobs: run.jobs.into(),
        max_requests: run.max_requests as usize,
        min_confidence: run.min_confidence,
    };
    let future = tenet_engine::run(&evidence, options, &evaluator, &review, &emit);
    tokio::pin!(future);
    let mut refresh = tokio::time::interval(std::time::Duration::from_millis(100));
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let live = reporter.borrow().is_live();
    let checked = loop {
        tokio::select! {
            result = &mut future => break result?,
            _ = refresh.tick(), if live => reporter.borrow_mut().refresh()?,
            signal = tokio::signal::ctrl_c() => {
                signal?;
                // Answers already received stay in the cache; an incomplete check adds no revision.
                let mut summary = reporter.borrow().summary.clone();
                summary.cancelled = true;
                emit(Event::Summary {
                    summary,
                    revision: None,
                    pending: Vec::new(),
                    exit_code: 130,
                })?;
                return Ok(130);
            }
        }
    };
    let request = CheckRequest {
        selection,
        context: &check.input.context,
        model: Some(&identity),
    };
    let revision = review.commit(&evidence, &request, &checked)?;
    finish(&emit, checked, revision)
}

fn finish(
    emit: &impl Fn(Event) -> Result<()>,
    checked: Checked,
    revision: Option<u64>,
) -> Result<u8> {
    let code = checked.exit_code();
    let pending = checked.pending();
    emit(Event::Summary {
        summary: checked.summary,
        revision,
        pending,
        exit_code: code,
    })?;
    Ok(code)
}
