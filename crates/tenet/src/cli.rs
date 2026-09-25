use clap::{Args, Parser, Subcommand, ValueEnum};
use ev_grep_jev::Provider;
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "Check code against Markdown contracts")]
pub(crate) struct Cli {
    #[arg(long, global = true, default_value = ".")]
    pub root: PathBuf,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Evaluate current file contents against contracts.
    Check(Check),
    /// Test contract examples through the same evaluator.
    Eval(Run),
    /// Validate contract documents without calling a model.
    Validate,
    /// Browse contract requirements and scope.
    Contracts {
        #[command(subcommand)]
        command: Contracts,
    },
}

#[derive(Subcommand)]
pub(crate) enum Contracts {
    List {
        #[arg(long = "for")]
        path: Option<PathBuf>,
    },
    View {
        name: String,
    },
}

#[derive(Args)]
pub(crate) struct Check {
    pub paths: Vec<PathBuf>,
    /// Select changed files, including working-tree changes; findings may predate the diff.
    #[arg(long)]
    pub changed_since: Option<String>,
    /// Show candidate pairs without calling a model.
    #[arg(long)]
    pub dry_run: bool,
    #[command(flatten)]
    pub run: Run,
}

#[derive(Args)]
pub(crate) struct Run {
    #[arg(long)]
    pub contract: Option<String>,
    #[arg(long, env = "TENET_PROVIDER", default_value = "openrouter")]
    pub provider: Provider,
    #[arg(long, env = "TENET_MODEL")]
    pub model: Option<String>,
    #[arg(long, default_value_t = 4, value_parser = clap::value_parser!(u16).range(1..=256))]
    pub jobs: u16,
    #[arg(long, default_value_t = 1000, value_parser = clap::value_parser!(u32).range(1..))]
    pub max_requests: u32,
    #[arg(long, value_enum, default_value_t = ReporterKind::Default)]
    pub reporter: ReporterKind,
    /// Shorthand for --reporter jsonl.
    #[arg(long)]
    pub json: bool,
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
pub(crate) enum ReporterKind {
    Default,
    Verbose,
    Jsonl,
}
