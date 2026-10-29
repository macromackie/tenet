//! The `tenet` command: it validates, adopts and checks contracts and keeps their review.

#![expect(
    clippy::multiple_crate_versions,
    reason = "Dependencies pin base64, cpufeatures, getrandom and windows-sys at two versions \
              each, as deny.toml's skip entries record."
)]

extern crate alloc;

mod adoption;
mod cli;
mod commands;
mod diagnostics;
mod error;
mod output;
mod progress;
mod provider;
mod report_format;
mod reporter;
mod review;
mod telemetry;

use std::{io, process::ExitCode};

use clap::Parser as _;
use cli::{Cli, Command, ExitStatus, ReporterKind};
use error::CommandError;
use tracing::{Instrument as _, Span};

#[tokio::main(flavor = "current_thread")]
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "Only the commands that take `--json` print a JSON error."
)]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let json = match &cli.command {
        Command::Check(check) => check.run.json || check.run.reporter == ReporterKind::Jsonl,
        Command::Status { json, .. } | Command::Show { json, .. } | Command::Log { json, .. } => {
            *json
        },
        _ => false,
    };
    let traced = match &cli.trace {
        Some(path) => telemetry::install(path, cli.trace_environment),
        None => Ok(()),
    };
    let span = (matches!(cli.command, Command::Check(_)).then(telemetry::check))
        .unwrap_or_else(Span::none);
    let result = async {
        traced?;
        commands::execute(&cli).await
    }
    .instrument(span.clone())
    .await;
    let status = match result {
        Ok(status) => status,
        // A reader such as `head` closed the output early; it has what it wanted.
        Err(CommandError::Io(error)) if error.kind() == io::ErrorKind::BrokenPipe => {
            ExitStatus::Error
        },
        Err(error) => {
            span.in_scope(|| telemetry::record_stopped(&error));
            // JSON output carries the error too, but an agent may read only stderr.
            if json {
                drop(output::write_document(
                    &mut output::open_stdout(),
                    tenet_engine::EVENTS_VERSION,
                    serde_json::json!({
                        "type": "error",
                        "message": tenet_engine::Causes(&error).to_string(),
                        "exit_code": u8::from(ExitStatus::Error),
                    }),
                ));
            }
            drop(diagnostics::write_error(
                &cli.root,
                &error,
                &mut output::open_stderr(),
            ));
            ExitStatus::Error
        },
    };
    let _: &Span = span.record("exit_code", u8::from(status));
    ExitCode::from(status)
}
