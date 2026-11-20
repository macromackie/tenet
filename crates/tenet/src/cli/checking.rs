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
#[expect(
    clippy::struct_excessive_bools,
    reason = "These are independent user-facing CLI flags parsed by clap."
)]
pub(crate) struct Check {
    #[command(flatten)]
    pub input: Input,
    /// Check the full scope even in pull request CI.
    #[arg(long, conflicts_with_all = ["base", "snapshot", "local", "qualify"])]
    pub full: bool,
    /// Run trusted project checks without a model. Optionally select CONTRACT/NAME.
    #[arg(
        hide_short_help = true,
        help_heading = "Project checks",
        long,
        num_args = 0..=1,
        default_missing_value = "",
        value_name = "CONTRACT/NAME",
          conflicts_with_all = ["paths", "base", "snapshot", "patch", "contract", "context",
                                "dry_run", "context_profile", "max_requests", "jobs",
                                "contract_batch_size", "min_confidence", "reporter"])]
    pub local: Option<String>,
    /// Exercise a project checker against declared good, defective and refactored controls.
    #[arg(
        hide_short_help = true,
        help_heading = "Project checks",
        long,
        value_name = "CONTRACT/NAME",
        conflicts_with = "final_check",
          conflicts_with_all = ["local", "paths", "base", "snapshot", "patch", "contract",
                                "context", "dry_run", "fresh", "context_profile",
                                "max_requests", "jobs", "contract_batch_size", "min_confidence",
                                "reporter"])]
    pub qualify: Option<String>,
    /// Verify the final selected source with both semantic review and trusted project checks.
    #[arg(long = "final", conflicts_with_all = ["local", "dry_run", "snapshot", "patch"])]
    pub final_check: bool,
    /// Verify receipts from a trusted external checker runner instead of executing project code.
    #[arg(
        hide_short_help = true,
        help_heading = "Project checks",
        long,
        value_name = "FILE",
        requires = "final_check"
    )]
    pub evidence: Option<PathBuf>,
    /// Show candidate pairs and predict requests without calling a model or updating the review.
    #[arg(hide_short_help = true, help_heading = "Planning and replay", long)]
    pub dry_run: bool,
    /// Rerun the selected checks even when their saved results can be reused.
    #[arg(hide_short_help = true, help_heading = "Planning and replay", long)]
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
    /// Name of the contract to settle.
    pub contract: String,
    /// The selected paths of the check to settle, when the contract stands in several.
    pub paths: Vec<PathBuf>,
    /// Conclusion to record: verified, preserved, unaffected, clear, failed, or unresolved.
    #[arg(long)]
    pub conclusion: String,
    /// Code that supports the conclusion, as PATH, PATH:LINE, or PATH:START-END; repeat as needed.
    #[arg(long, value_name = "PATH[:START-END]")]
    pub evidence: Vec<SpanRequest>,
    /// What the evidence shows, or for unresolved, the fact that is missing.
    #[arg(long, short)]
    pub message: String,
    /// Print the recorded conclusion as a JSON document.
    #[arg(long)]
    pub json: bool,
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
    /// Files or directories to check; checks all governed files when omitted.
    pub paths: Vec<PathBuf>,
    /// Bound assessment inputs for the standard or opt-in long context window.
    #[arg(
        hide_short_help = true,
        help_heading = "Model settings",
        long,
        value_enum,
        default_value_t = ContextWindow::Standard,
    )]
    pub context_profile: ContextWindow,
    /// Supply a related repository file, or only some of its lines, without changing contract
    /// scope; repeat as needed.
    #[arg(
        hide_short_help = true,
        help_heading = "Planning and replay",
        long,
        value_name = "PATH[:START-END]"
    )]
    pub context: Vec<SpanRequest>,
    /// Compare this exact revision with the working tree, assuming base compliance.
    #[arg(long, conflicts_with = "snapshot")]
    pub base: Option<String>,
    /// Check an isolated copy of this directory, optionally applying a patch.
    #[arg(
        hide_short_help = true,
        help_heading = "Planning and replay",
        long,
        conflicts_with = "base"
    )]
    pub snapshot: Option<PathBuf>,
    /// Unified diff file to apply inside the snapshot directory.
    #[arg(
        hide_short_help = true,
        help_heading = "Planning and replay",
        long,
        requires = "snapshot"
    )]
    pub patch: Option<PathBuf>,
    /// Limit the check to one contract by name.
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
    /// Model provider backend (openrouter or typesafe).
    #[arg(
        hide_short_help = true,
        help_heading = "Model settings",
        long,
        env = "TENET_PROVIDER",
        default_value = "openrouter"
    )]
    pub provider: Provider,
    /// Model identifier used for semantic contract assessment.
    #[arg(
        hide_short_help = true,
        help_heading = "Model settings",
        long,
        env = "TENET_MODEL",
        default_value_if("provider", "openrouter", crate::provider::OPENROUTER_MODEL),
        default_value_if("provider", "typesafe", crate::provider::TYPESAFE_MODEL)
    )]
    pub model: Option<String>,
    /// Send requests and the provider credential to a trusted protocol-compatible proxy.
    #[arg(
        hide_short_help = true,
        help_heading = "Model settings",
        long,
        env = "TENET_ENDPOINT"
    )]
    pub endpoint: Option<String>,
    /// Maximum number of concurrent model requests.
    #[arg(
        hide_short_help = true,
        help_heading = "Model settings",
        long,
        default_value_t = 4,
        value_parser = clap::value_parser!(u16).range(1..=256),
    )]
    pub jobs: u16,
    /// Share file context across at most this many contracts in one model request.
    #[arg(
        hide_short_help = true,
        help_heading = "Model settings",
        long,
        default_value_t = 1,
        value_parser = clap::value_parser!(u16).range(1..=64),
    )]
    pub contract_batch_size: u16,
    /// Maximum model requests. Zero reuses cached answers and leaves unaffordable work pending.
    #[arg(long, default_value_t = 1000)]
    pub max_requests: u32,
    /// Route lower-confidence answers to unresolved while preserving their raw values.
    #[arg(hide_short_help = true, help_heading = "Model settings",
        long,
        default_value_t = tenet_engine::MIN_CONFIDENCE,
        value_parser = parse_min_confidence
    )]
    pub min_confidence: f64,
    /// Output format for check results (default, verbose, or jsonl).
    #[arg(
        hide_short_help = true,
        help_heading = "Output",
        long,
        value_enum,
        default_value_t = ReporterKind::Default,
    )]
    pub reporter: ReporterKind,
    /// Write JSON Lines for ordinary checks, or one JSON document for a final check.
    #[arg(long)]
    pub json: bool,
}

#[derive(Clone, Copy, ValueEnum)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order."
)]
pub(crate) enum ContextWindow {
    Standard,
    Long,
}

impl ContextWindow {
    pub(crate) const fn read_profile(self) -> tenet_engine::ContextProfile {
        match self {
            Self::Standard => tenet_engine::ContextProfile::Standard,
            Self::Long => tenet_engine::ContextProfile::Long,
        }
    }
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
