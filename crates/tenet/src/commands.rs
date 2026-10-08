use std::{
    cell::RefCell,
    io::{self, Write},
    path::Component,
};

use anyhow::{Context, Result, ensure};
use ev_grep_jev::Jev;
use tenet_engine::{Event, Mode, Options, PreparedSnapshot, RunInfo, Selection};

use crate::{
    cli::{Cli, Command, Contracts, ReporterKind, Run},
    reporter::Reporter,
};

pub(crate) async fn execute(cli: &Cli) -> Result<u8> {
    match &cli.command {
        Command::Validate => {
            let contracts = tenet_engine::discover_contracts(&cli.root)?;
            ensure!(!contracts.is_empty(), "no contracts found");
            writeln!(io::stdout(), "{} contracts valid", contracts.len())?;
            Ok(0)
        }
        Command::Contracts { command } => {
            let contracts = tenet_engine::discover_contracts(&cli.root)?;
            match command {
                Contracts::List { path, base, json } => {
                    if let Some(path) = path {
                        ensure!(
                            path.components()
                                .all(|c| matches!(c, Component::Normal(_) | Component::CurDir)),
                            "--for path must be relative to the project root"
                        );
                    }
                    let plan = base
                        .as_ref()
                        .map(|base| {
                            tenet_engine::plan(
                                &cli.root,
                                &Selection {
                                    base: Some(base.clone()),
                                    ..Selection::default()
                                },
                            )
                        })
                        .transpose()?;
                    let contracts = plan
                        .as_ref()
                        .map_or(contracts.as_slice(), |plan| plan.contracts.as_slice());
                    let selected: Vec<_> = contracts
                        .iter()
                        .filter(|c| path.as_ref().is_none_or(|p| p.starts_with(&c.scope)))
                        .collect();
                    if *json {
                        let entries: Vec<_> = selected.iter().map(|c| serde_json::json!({"name":c.name,"path":c.path,"scope":c.scope,"message":c.message})).collect();
                        writeln!(
                            io::stdout(),
                            "{}",
                            serde_json::json!({"version":1,"contracts":entries,"contract_changes":plan.as_ref().map(|p| p.contract_changes.as_slice()).unwrap_or(&[])})
                        )?;
                    } else {
                        for c in selected {
                            writeln!(io::stdout(), "{}  {}", c.name, c.path.display())?;
                        }
                    }
                }
                Contracts::View { name } => {
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
            let snapshot = check
                .snapshot
                .as_ref()
                .map(|base| PreparedSnapshot::new(base, check.patch.as_deref()))
                .transpose()?;
            let root = snapshot
                .as_ref()
                .map_or(cli.root.as_path(), PreparedSnapshot::root);
            let selection = Selection {
                paths: check.paths.clone(),
                base: if check.patch.is_some() {
                    Some("HEAD".into())
                } else {
                    check.base.clone()
                },
                contract: check.run.contract.clone(),
            };
            let plan = tenet_engine::plan(root, &selection)?.with_context(&check.context)?;
            evaluate(&plan, &check.run, check.dry_run, snapshot.as_ref()).await
        }
    }
}

async fn evaluate(
    plan: &tenet_engine::Plan,
    run: &Run,
    dry: bool,
    snapshot: Option<&PreparedSnapshot>,
) -> Result<u8> {
    let kind = if run.json {
        ReporterKind::Jsonl
    } else {
        run.reporter
    };
    let reporter = RefCell::new(Reporter::new(kind, &plan.contracts));
    if dry {
        let mut errors = 0;
        for c in &plan.contracts {
            for path in plan.files.iter().filter(|p| {
                p.starts_with(&c.scope)
                    || plan
                        .previous_paths
                        .get(*p)
                        .is_some_and(|old| old.starts_with(&c.scope))
            }) {
                let input = plan
                    .change(path)
                    .and_then(|change| plan.file_input(c, &change));
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
                        serde_json::json!({"version":3,"type":"selected","contract":c.name,"scope":plan.requested_scope(c),"path":path,"contract_hash":c.hash,"input_bytes":input_bytes,"input_limit_bytes":tenet_engine::MAX_FILE_BYTES,"error":error})
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
                serde_json::json!({"version":3,"type":"summary","dry_run":true,"files":plan.files.len(),"mode":plan.mode,"assume_base_valid":plan.mode == Mode::Diff,"deleted":plan.deleted,"errors":errors,"exit_code":if errors > 0 { 2 } else { 0 }})
            )?;
        }
        return Ok(if errors > 0 { 2 } else { 0 });
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
    let emit = |event| reporter.borrow_mut().emit(event);
    emit(Event::Begin {
        run: RunInfo {
            version: env!("CARGO_PKG_VERSION").into(),
            root: plan.root.display().to_string(),
            provider: run.provider.to_string(),
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
        },
    })?;
    let options = Options {
        jobs: run.jobs.into(),
        max_requests: run.max_requests as usize,
        min_confidence: run.min_confidence,
    };
    let future = tenet_engine::run(plan, options, &evaluator, &emit);
    tokio::pin!(future);
    let mut refresh = tokio::time::interval(std::time::Duration::from_millis(100));
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let live = reporter.borrow().is_live();
    let summary = loop {
        tokio::select! {
            result = &mut future => break result?,
            _ = refresh.tick(), if live => reporter.borrow_mut().refresh()?,
            signal = tokio::signal::ctrl_c() => {
                signal?;
                let mut summary = reporter.borrow().summary.clone();
                summary.cancelled = true;
                break summary;
            }
        }
    };
    let code = summary.exit_code();
    emit(Event::Summary {
        summary,
        exit_code: code,
    })?;
    Ok(code)
}
