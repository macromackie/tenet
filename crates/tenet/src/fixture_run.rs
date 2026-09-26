use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, ensure};
use ev_grep_core::Evaluator;
use ev_grep_jev::Jev;
use serde_json::{Value, json};
use tenet_engine::{Event, Options, Summary};

use crate::{
    cli::{Eval, ReporterKind},
    fixture_report::{CaseResult, Display},
    fixtures::{self, FixtureCase, Prepared},
};

struct Record {
    file: File,
    stdout: bool,
}

impl Record {
    fn emit(&mut self, value: &Value) -> Result<()> {
        let line = redact(serde_json::to_string(value)?);
        writeln!(self.file, "{line}")?;
        self.file.flush()?;
        if self.stdout {
            writeln!(io::stdout(), "{line}")?;
            io::stdout().flush()?;
        }
        Ok(())
    }
}

fn redact(mut text: String) -> String {
    for name in ["OPENROUTER_API_KEY", "TYPESAFE_API_KEY"] {
        if let Ok(key) = std::env::var(name)
            && !key.is_empty()
        {
            text = text.replace(&key, "[REDACTED]");
        }
    }
    text
}

pub(crate) async fn assess(
    case: &FixtureCase,
    prepared: &Prepared,
    options: Options,
    evaluator: &impl Evaluator,
    emit: &impl Fn(Event) -> Result<()>,
) -> Result<CaseResult> {
    let actual = RefCell::new(BTreeMap::new());
    let summary = tenet_engine::run(&prepared.plan, options, evaluator, &|event| {
        if let Event::ContractFinished {
            contract,
            conclusion: Some(conclusion),
            ..
        } = &event
        {
            actual
                .borrow_mut()
                .insert(contract.clone(), conclusion.status);
        }
        emit(event)
    })
    .await?;
    let error = if summary.errors > 0 || summary.incomplete > 0 {
        Some("File or repository checks could not complete; see the recorded events.".into())
    } else if actual.borrow().len() != case.case.expect.len() {
        Some("Missing contract conclusions.".into())
    } else {
        None
    };
    Ok(CaseResult {
        case: case.id.clone(),
        expected: case.case.expect.clone(),
        actual: actual.into_inner(),
        error,
        summary,
    })
}

pub(crate) async fn execute(path: &Path, args: &Eval) -> Result<u8> {
    let cases = fixtures::discover(path, args)?;
    let kind = if args.run.json {
        ReporterKind::Jsonl
    } else {
        args.run.reporter
    };
    if args.validate {
        for case in &cases {
            case.prepare()?;
        }
        if kind == ReporterKind::Jsonl {
            println!(
                "{}",
                json!({"version":1,"type":"fixtures_validated","cases":cases.len()})
            );
        } else {
            println!("{} fixture cases valid", cases.len());
        }
        return Ok(0);
    }
    let model = args.run.provider.model(args.run.model.as_deref())?;
    let key = std::env::var(args.run.provider.key_variable()).with_context(|| {
        format!(
            "set {} to run evaluations",
            args.run.provider.key_variable()
        )
    })?;
    ensure!(!key.is_empty(), "provider key is empty");
    let evaluator = Jev::with_endpoint(
        args.run.provider,
        &model,
        &key,
        args.run
            .endpoint
            .as_deref()
            .unwrap_or(args.run.provider.endpoint()),
    )?;
    let destination = if let Some(path) = &args.output {
        path.clone()
    } else {
        PathBuf::from(".tenet/evals").join(format!(
            "{}-{}.jsonl",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
            std::process::id()
        ))
    };
    if let Some(parent) = destination.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let record = RefCell::new(Record {
        file: File::create_new(&destination)
            .context("cannot create eval report (existing reports are not overwritten)")?,
        stdout: kind == ReporterKind::Jsonl,
    });
    let display = RefCell::new(Display::new(kind, cases.len()));
    let binary_hash = blake3::hash(&fs::read(std::env::current_exe()?)?)
        .to_hex()
        .to_string();
    record.borrow_mut().emit(&json!({"version":1,"type":"eval_begin","cases":cases.len(),"provider":args.run.provider.to_string(),"model":model,"binary_blake3":binary_hash,"split":args.split,"max_requests":args.run.max_requests,"jobs":args.run.jobs}))?;
    let started = Instant::now();
    for case in &cases {
        display.borrow_mut().start(&case.id)?;
        let case_started = Instant::now();
        let remaining =
            (args.run.max_requests as usize).saturating_sub(display.borrow().totals.requests);
        let future = async {
            ensure!(remaining > 0, "evaluation request budget exhausted");
            let prepared = case.prepare()?;
            record.borrow_mut().emit(&json!({"version":1,"type":"case_begin","case":case.id,"fixture_blake3":prepared.hash,"mode":prepared.plan.mode,"expected":case.case.expect,"snapshot":prepared.snapshot.snapshot,"patch_hash":prepared.snapshot.patch_hash}))?;
            assess(
                case,
                &prepared,
                Options {
                    jobs: args.run.jobs.into(),
                    max_requests: remaining,
                },
                &evaluator,
                &|event| {
                    display.borrow_mut().event(&event)?;
                    record.borrow_mut().emit(
                        &json!({"version":1,"type":"case_event","case":case.id,"event":event}),
                    )
                },
            )
            .await
        };
        tokio::pin!(future);
        let mut refresh = tokio::time::interval(std::time::Duration::from_millis(100));
        refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let live = display.borrow().live();
        let result = loop {
            tokio::select! {
                result = &mut future => break Some(result),
                _ = refresh.tick(), if live => display.borrow_mut().refresh()?,
                signal = tokio::signal::ctrl_c() => { signal?; break None; }
            }
        };
        let Some(result) = result else {
            display.borrow_mut().totals.cancelled = true;
            break;
        };
        let result = match result {
            Ok(result) => result,
            Err(error) => CaseResult {
                case: case.id.clone(),
                expected: case.case.expect.clone(),
                actual: BTreeMap::new(),
                error: Some(redact(format!("{error:#}"))),
                summary: Summary {
                    elapsed_ms: case_started.elapsed().as_millis(),
                    ..Summary::default()
                },
            },
        };
        record.borrow_mut().emit(
            &json!({"version":1,"type":"case_finished","matched":result.matched(),"result":result}),
        )?;
        display.borrow_mut().finish(&result)?;
    }
    let code = display.borrow().totals.exit_code(args.strict);
    record.borrow_mut().emit(&json!({"version":1,"type":"eval_summary","summary":display.borrow().totals,"elapsed_ms":started.elapsed().as_millis(),"exit_code":code}))?;
    display.borrow_mut().summary()?;
    if kind != ReporterKind::Jsonl {
        eprintln!(" Report        {}", destination.display());
    }
    Ok(code)
}
