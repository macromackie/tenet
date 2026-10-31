use core::num::TryFromIntError;
use std::path::PathBuf;

use clap::{Args, ValueEnum};
use ev_grep_jev::Provider;
use tenet_engine::SpanRequest;

#[derive(Args)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order, and ExitStatus follows its exit \
              codes."
)]
pub(crate) struct Check {
    #[command(flatten)]
    pub input: Input,
    /// Show candidate pairs and predict requests without calling a model or updating the review.
    #[arg(long, conflicts_with = "no_model")]
    pub dry_run: bool,
    /// Capture the change and reuse earlier conclusions for unchanged inputs; leave the rest
    /// pending.
    #[arg(long, conflicts_with = "fresh")]
    pub no_model: bool,
    /// Ask the model again instead of reusing answers to identical earlier requests.
    #[arg(long)]
    pub fresh: bool,
    #[command(flatten)]
    pub run: Run,
}

#[derive(Args)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order, and ExitStatus follows its exit \
              codes."
)]
pub(crate) struct Record {
    pub contract: String,
    /// The selected paths of the check to settle, when the contract stands in several.
    pub paths: Vec<PathBuf>,
    /// One of the conclusions a check of this contract's mode and scope can reach.
    #[arg(long)]
    pub conclusion: String,
    /// Code that supports the conclusion, as PATH, PATH:LINE, or PATH:START-END; repeat as needed.
    #[arg(long, value_name = "PATH[:START-END]")]
    pub evidence: Vec<SpanRequest>,
    /// What the evidence shows, or for unresolved, the fact that is missing.
    #[arg(long, short)]
    pub message: String,
}

#[derive(Args)]
pub(crate) struct Export {
    #[command(flatten)]
    pub input: Input,
    /// Maximum encoded packet size in bytes; oversized input is rejected, never truncated.
    #[arg(
        long,
        default_value_t = 1_048_576,
        value_parser = clap::value_parser!(u32).range(1..=67_108_864)
    )]
    pub max_bytes: u32,
}

#[derive(Args)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order, and ExitStatus follows its exit \
              codes."
)]
pub(crate) struct Input {
    pub paths: Vec<PathBuf>,
    /// Supply a related repository file, or only some of its lines, without changing contract
    /// scope; repeat as needed.
    #[arg(long, value_name = "PATH[:START-END]")]
    pub context: Vec<SpanRequest>,
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
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order, and ExitStatus follows its exit \
              codes."
)]
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
    #[arg(
        long,
        default_value_t = tenet_engine::MIN_CONFIDENCE,
        value_parser = parse_min_confidence
    )]
    pub min_confidence: f64,
    #[arg(long, value_enum, default_value_t = ReporterKind::Default)]
    pub reporter: ReporterKind,
    /// Shorthand for --reporter jsonl.
    #[arg(long)]
    pub json: bool,
}

impl Run {
    /// The parsed request budget in the engine's count type.
    pub(crate) fn request_budget(&self) -> Result<usize, TryFromIntError> {
        usize::try_from(self.max_requests)
    }
}

#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
fn parse_min_confidence(value: &str) -> Result<f64, String> {
    let value: f64 = value
        .parse()
        .map_err(|error| format!("expected a number between 0 and 1: {error:#}"))?;
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err("min-confidence must be between 0 and 1".into())
    }
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order, and ExitStatus follows its exit \
              codes."
)]
pub(crate) enum ReporterKind {
    Default,
    Verbose,
    Jsonl,
}
