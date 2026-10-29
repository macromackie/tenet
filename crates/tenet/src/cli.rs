use std::{path::PathBuf, process::ExitCode};

use clap::{Args, Parser, Subcommand, ValueEnum};
use ev_grep_jev::Provider;
use tenet_engine::{
    SpanRequest,
    review::{DIRECTORY, OverallStatus},
};

use crate::telemetry::Environment;

#[derive(Parser)]
#[command(version, about = "Check code against Markdown contracts")]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order, and ExitStatus follows its exit \
              codes."
)]
pub(crate) struct Cli {
    #[arg(long, global = true, default_value = ".")]
    pub root: PathBuf,
    /// Keep review state here instead of `.tenet` in the project root, such as when the checkout
    /// is read-only.
    #[arg(long, global = true, env = "TENET_STATE_DIR")]
    pub state_dir: Option<PathBuf>,
    /// Write a JSON Lines trace of spans and events to PATH, or to stderr with '-'.
    #[arg(long, global = true, env = "TENET_TRACE", value_name = "PATH")]
    pub trace: Option<PathBuf>,
    /// Environment that the trace's first record names; the default is ci when CI is set,
    /// otherwise local.
    #[arg(
        long,
        global = true,
        env = "TENET_TRACE_ENVIRONMENT",
        value_enum,
        default_value_t = Environment::detect(),
        hide_default_value = true
    )]
    pub trace_environment: Environment,
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Where `check` keeps review state and the other review commands read it.
    pub(crate) fn resolve_state(&self) -> PathBuf {
        (self.state_dir.clone()).unwrap_or_else(|| self.root.join(DIRECTORY))
    }
}

/// How a command ended.
///
/// Its conversion to `u8` is the one place each exit status's number is
/// written. `check` and `status` end with a review's overall status; `outdated` and `update` fail
/// when a copy needs attention.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order, and ExitStatus follows its exit \
              codes."
)]
pub(crate) enum ExitStatus {
    /// Every contract is settled without a violation, or the command succeeded.
    Passed,
    /// A settled violation, or an installed copy the user has to resolve.
    Failed,
    /// Invalid input or configuration, or another execution error.
    Error,
    /// A recorded unresolved conclusion.
    Unresolved,
    /// A contract is pending.
    Pending,
    /// The command was cancelled.
    Cancelled,
}

impl From<OverallStatus> for ExitStatus {
    fn from(status: OverallStatus) -> Self {
        match status {
            OverallStatus::Passed => Self::Passed,
            OverallStatus::Unresolved => Self::Unresolved,
            OverallStatus::Failed => Self::Failed,
            OverallStatus::Pending => Self::Pending,
            OverallStatus::Error => Self::Error,
            OverallStatus::Cancelled => Self::Cancelled,
        }
    }
}

/// The number that JSON output's `exit_code` reports and the process returns.
impl From<ExitStatus> for u8 {
    #[inline]
    fn from(status: ExitStatus) -> Self {
        match status {
            ExitStatus::Passed => 0,
            ExitStatus::Failed => 1,
            ExitStatus::Error => 2,
            ExitStatus::Unresolved => 3,
            ExitStatus::Pending => 4,
            ExitStatus::Cancelled => 130,
        }
    }
}

impl From<ExitStatus> for ExitCode {
    #[inline]
    fn from(status: ExitStatus) -> Self {
        Self::from(u8::from(status))
    }
}

#[derive(Subcommand)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order, and ExitStatus follows its exit \
              codes."
)]
pub(crate) enum Command {
    /// Assess a repository, or whether a change preserves its contracts, and update the review.
    Check(Check),
    /// Show which contracts are settled and which are pending, without calling a model.
    Status {
        /// Report the diff review against this commit instead of the latest check's review.
        #[arg(long)]
        base: Option<String>,
        #[arg(long)]
        json: bool,
        /// First remove cached answers this Tenet cannot reuse: those an earlier Tenet wrote and
        /// those a later answer to the same request replaced.
        #[arg(long)]
        prune_cache: bool,
    },
    /// Show what a reviewer needs to settle one contract: rule, change, model answer, and history.
    Show {
        #[arg(required_unless_present = "pending")]
        contract: Option<String>,
        /// The selected paths of the check to show, when the contract stands in several.
        paths: Vec<PathBuf>,
        /// Show every pending contract, printing their shared change once.
        #[arg(long, conflicts_with = "contract")]
        pending: bool,
        #[arg(long)]
        json: bool,
    },
    /// Record a conclusion for a contract's current input, with the evidence that supports it.
    Record(Record),
    /// List checks and records, newest first.
    Log {
        contract: Option<String>,
        /// Print the matching review events as JSON Lines.
        #[arg(long)]
        json: bool,
    },
    /// Export captured contracts and source as JSON without calling a model.
    Export(Export),
    /// Validate contract documents without calling a model.
    Validate,
    /// Browse contract requirements and scope, and install shared contracts.
    Contracts {
        #[command(subcommand)]
        command: Contracts,
    },
    /// Install and update shared agent skills through .tenet.lock.
    Skills {
        #[command(subcommand)]
        command: Skills,
    },
}

#[derive(Subcommand)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order, and ExitStatus follows its exit \
              codes."
)]
pub(crate) enum Contracts {
    List {
        /// List only the contracts that govern this file.
        #[arg(long = "for", value_name = "FILE")]
        path: Option<PathBuf>,
        /// Select scopes touched since this revision, including contract changes.
        #[arg(long)]
        base: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Show {
        name: String,
    },
    /// Install a shared contract and record it in .tenet.lock.
    Add {
        #[arg(
            value_name = "SOURCE/NAME[@REF]",
            help = "Library contract; without a ref, the highest vX.Y.Z tag"
        )]
        spec: String,
        /// Directory whose .contracts receives the next free number.
        #[arg(long, required_unless_present = "path", conflicts_with = "path")]
        scope: Option<PathBuf>,
        /// Record an existing CONTRACT.md instead of writing a new copy.
        #[arg(long)]
        path: Option<PathBuf>,
        /// Local contract name when the upstream name is already used.
        #[arg(long, conflicts_with = "path")]
        name: Option<String>,
        #[arg(long, conflicts_with = "path", value_parser = clap::value_parser!(u32).range(1..))]
        number: Option<u32>,
        /// Register a source as ALIAS=URL; repeat as needed.
        #[arg(long = "source")]
        sources: Vec<String>,
    },
    #[command(flatten)]
    Locked(Locked),
}

#[derive(Subcommand)]
pub(crate) enum Skills {
    /// Install a shared skill folder and record it in .tenet.lock.
    Add {
        #[arg(
            value_name = "SOURCE/NAME[@REF]",
            help = "Library skill; without a ref, the highest vX.Y.Z tag"
        )]
        spec: String,
        /// Directory that receives the skill folder.
        #[arg(long, default_value = ".agents/skills", conflicts_with = "path")]
        dir: PathBuf,
        /// Local folder and skill name when the upstream name is already used.
        #[arg(long, conflicts_with = "path")]
        name: Option<String>,
        /// Record an existing skill folder instead of writing a new copy.
        #[arg(long)]
        path: Option<PathBuf>,
        /// Register a source as ALIAS=URL; repeat as needed.
        #[arg(long = "source")]
        sources: Vec<String>,
    },
    #[command(flatten)]
    Locked(Locked),
}

/// Commands for copies recorded in .tenet.lock, the same for contracts and skills.
#[derive(Subcommand)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order, and ExitStatus follows its exit \
              codes."
)]
pub(crate) enum Locked {
    /// Compare installed copies with their sources.
    Outdated {
        /// Report only local edits, without fetching sources.
        #[arg(long)]
        offline: bool,
        #[arg(long)]
        json: bool,
    },
    /// Take upstream changes, merging local Markdown edits three ways.
    Update {
        /// Installed copies to update; all when omitted.
        paths: Vec<PathBuf>,
        /// Move to this tag, branch, or commit instead of the newest.
        #[arg(long = "ref")]
        reference: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show upstream changes and local edits since the locked version.
    Diff {
        path: PathBuf,
        /// Print the base, upstream, and local texts.
        #[arg(long)]
        json: bool,
    },
    /// Remove an installed copy and its lock entry.
    Remove {
        path: PathBuf,
        /// Keep the files as ordinary local files.
        #[arg(long)]
        keep_file: bool,
    },
}

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
