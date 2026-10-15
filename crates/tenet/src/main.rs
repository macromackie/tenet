mod adoption;
mod cli;
mod commands;
mod diagnostics;
mod progress;
mod report_format;
mod reporter;
mod review;

use clap::Parser;
use cli::{Cli, Command, ReporterKind};
use std::{
    io::{self, Write},
    process::ExitCode,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let json = match &cli.command {
        Command::Check(c) => c.run.json || c.run.reporter == ReporterKind::Jsonl,
        Command::Status { json, .. } | Command::Show { json, .. } | Command::Log { json, .. } => {
            *json
        }
        _ => false,
    };
    match commands::execute(&cli).await {
        Ok(code) => ExitCode::from(code),
        // A reader such as `head` closed the output early; it has what it wanted.
        Err(error)
            if error.chain().any(|cause| {
                cause
                    .downcast_ref::<io::Error>()
                    .is_some_and(|error| error.kind() == io::ErrorKind::BrokenPipe)
            }) =>
        {
            ExitCode::from(2)
        }
        Err(error) => {
            if json {
                let _ = writeln!(
                    io::stdout(),
                    "{}",
                    serde_json::json!({"version": 4, "type": "error", "message": format!("{error:#}"), "exit_code": 2})
                );
            } else {
                let _ = diagnostics::error(&cli.root, &error, &mut io::stderr());
            }
            ExitCode::from(2)
        }
    }
}
