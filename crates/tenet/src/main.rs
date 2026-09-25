mod cli;
mod commands;
mod diagnostics;
mod reporter;

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
        Command::Eval(r) => r.json || r.reporter == ReporterKind::Jsonl,
        _ => false,
    };
    match commands::execute(&cli).await {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            if json {
                let _ = writeln!(
                    io::stdout(),
                    "{}",
                    serde_json::json!({"version": 1, "type": "error", "message": error.to_string(), "exit_code": 2})
                );
            } else {
                let _ = diagnostics::error(&cli.root, &error, &mut io::stderr());
            }
            ExitCode::from(2)
        }
    }
}
