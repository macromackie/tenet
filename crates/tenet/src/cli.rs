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
    /// Assess a repository, or whether a change preserves its contracts.
    Check(Check),
    /// Export captured contracts and source as JSON without calling a model.
    Evidence(Evidence),
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
        /// Select scopes touched since this revision, including contract changes.
        #[arg(long)]
        base: Option<String>,
        #[arg(long)]
        json: bool,
    },
    View {
        name: String,
    },
}

#[derive(Args)]
pub(crate) struct Check {
    #[command(flatten)]
    pub input: Input,
    /// Show candidate pairs without calling a model.
    #[arg(long)]
    pub dry_run: bool,
    #[command(flatten)]
    pub run: Run,
}

#[derive(Args)]
pub(crate) struct Evidence {
    #[command(flatten)]
    pub input: Input,
    /// Maximum encoded packet size in bytes; oversized input is rejected, never truncated.
    #[arg(long, default_value_t = 1_048_576, value_parser = clap::value_parser!(u32).range(1..=67_108_864))]
    pub max_bytes: u32,
}

#[derive(Args)]
pub(crate) struct Input {
    pub paths: Vec<PathBuf>,
    /// Supply related repository files without changing contract scope; repeat as needed.
    #[arg(long)]
    pub context: Vec<PathBuf>,
    /// Compare this exact revision with the working tree, assuming base compliance.
    #[arg(long, conflicts_with = "snapshot")]
    pub base: Option<String>,
    /// Check an isolated copy of this directory, optionally applying a patch.
    #[arg(long, conflicts_with = "base")]
    pub snapshot: Option<PathBuf>,
    #[arg(long, requires = "snapshot")]
    pub patch: Option<PathBuf>,
    #[arg(long)]
    pub contract: Option<String>,
}

#[derive(Args)]
pub(crate) struct Run {
    #[arg(long, env = "TENET_PROVIDER", default_value = "openrouter")]
    pub provider: Provider,
    #[arg(long, env = "TENET_MODEL")]
    pub model: Option<String>,
    /// Send requests and the provider credential to a trusted protocol-compatible proxy.
    #[arg(long, env = "TENET_ENDPOINT")]
    pub endpoint: Option<String>,
    #[arg(long, default_value_t = 4, value_parser = clap::value_parser!(u16).range(1..=256))]
    pub jobs: u16,
    #[arg(long, default_value_t = 1000, value_parser = clap::value_parser!(u32).range(1..))]
    pub max_requests: u32,
    /// Route lower-confidence answers to unresolved while preserving their raw values.
    #[arg(long, default_value_t = tenet_engine::MIN_CONFIDENCE, value_parser = min_confidence)]
    pub min_confidence: f64,
    #[arg(long, value_enum, default_value_t = ReporterKind::Default)]
    pub reporter: ReporterKind,
    /// Shorthand for --reporter jsonl.
    #[arg(long)]
    pub json: bool,
}

fn min_confidence(value: &str) -> Result<f64, String> {
    let value: f64 = value
        .parse()
        .map_err(|_| "expected a number between 0 and 1")?;
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err("min-confidence must be between 0 and 1".into())
    }
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
pub(crate) enum ReporterKind {
    Default,
    Verbose,
    Jsonl,
}
