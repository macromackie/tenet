use std::{
    cell::RefCell,
    io::{self, Write},
    path::Component,
};

use anyhow::{Context, Result, ensure};
use ev_grep_jev::Jev;
use tenet_engine::{Event, Options, RunInfo, Selection};

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
                Contracts::List { path } => {
                    if let Some(path) = path {
                        ensure!(
                            path.components()
                                .all(|c| matches!(c, Component::Normal(_) | Component::CurDir)),
                            "--for path must be relative to the project root"
                        );
                    }
                    for c in contracts
                        .iter()
                        .filter(|c| path.as_ref().is_none_or(|p| p.starts_with(&c.scope)))
                    {
                        writeln!(io::stdout(), "{}  {}", c.name, c.path.display())?;
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
            let selection = Selection {
                paths: check.paths.clone(),
                changed_since: check.changed_since.clone(),
                contract: check.run.contract.clone(),
            };
            let plan = tenet_engine::plan(&cli.root, &selection)?;
            evaluate(&plan, &check.run, false, check.dry_run).await
        }
        Command::Eval(run) => {
            let plan = tenet_engine::plan(
                &cli.root,
                &Selection {
                    contract: run.contract.clone(),
                    ..Selection::default()
                },
            )?;
            evaluate(&plan, run, true, false).await
        }
    }
}

async fn evaluate(plan: &tenet_engine::Plan, run: &Run, evaluation: bool, dry: bool) -> Result<u8> {
    let kind = if run.json {
        ReporterKind::Jsonl
    } else {
        run.reporter
    };
    let reporter = RefCell::new(Reporter::new(kind, &plan.contracts, evaluation));
    if dry {
        for c in &plan.contracts {
            for path in plan.files.iter().filter(|p| p.starts_with(&c.scope)) {
                if kind == ReporterKind::Jsonl {
                    writeln!(
                        io::stdout(),
                        "{}",
                        serde_json::json!({"version":1,"type":"selected","contract":c.name,"path":path,"contract_hash":c.hash})
                    )?;
                } else {
                    writeln!(io::stdout(), "{}  {}", c.name, path.display())?;
                }
            }
        }
        if kind == ReporterKind::Jsonl {
            writeln!(
                io::stdout(),
                "{}",
                serde_json::json!({"version":1,"type":"summary","dry_run":true,"files":plan.files.len(),"broadened":plan.broadened,"deleted":plan.deleted})
            )?;
        }
        return Ok(0);
    }
    let model = run.provider.model(run.model.as_deref())?;
    let key = std::env::var(run.provider.key_variable()).with_context(|| {
        format!(
            "set {} to run model evaluations",
            run.provider.key_variable()
        )
    })?;
    let evaluator = Jev::new(run.provider, &model, &key)?;
    let emit = |event| reporter.borrow_mut().emit(event);
    emit(Event::Begin {
        run: RunInfo {
            version: env!("CARGO_PKG_VERSION").into(),
            root: plan.root.display().to_string(),
            provider: run.provider.to_string(),
            model,
            mode: if evaluation { "eval" } else { "check" }.into(),
            base: plan.base.clone(),
            head: plan.head.clone(),
            broadened: plan.broadened,
            jobs: run.jobs.into(),
            max_requests: run.max_requests as usize,
        },
    })?;
    let options = Options {
        jobs: run.jobs.into(),
        max_requests: run.max_requests as usize,
    };
    let future = async {
        if evaluation {
            tenet_engine::run_examples(plan, options, &evaluator, &emit).await
        } else {
            tenet_engine::run(plan, options, &evaluator, &emit).await
        }
    };
    let summary = tokio::select! {
        result = future => result?,
        signal = tokio::signal::ctrl_c() => {
            signal?;
            let mut summary = reporter.borrow().summary.clone();
            summary.cancelled = true;
            summary
        }
    };
    let code = summary.exit_code(evaluation);
    emit(Event::Summary {
        summary,
        exit_code: code,
    })?;
    Ok(code)
}
