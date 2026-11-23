mod adoption;
mod checking;
mod exit_status;
mod onboarding;

use std::path::PathBuf;

pub(crate) use adoption::{Contracts, Locked, Skills};
pub(crate) use checking::{Check, Export, Input, Record, ReporterKind};
use clap::{Parser, Subcommand};
pub(crate) use exit_status::ExitStatus;
pub(crate) use onboarding::{Audit, Fix, Init, Scan};
use tenet_engine::review::DIRECTORY;

use crate::telemetry::Environment;

#[derive(Parser)]
#[command(version, about = "Check code against Markdown contracts")]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order, and ExitStatus follows its exit \
              codes."
)]
pub(crate) struct Cli {
    /// Project root directory to inspect.
    #[arg(
        long,
        global = true,
        default_value = ".",
        help_heading = "Global options"
    )]
    pub root: PathBuf,
    /// Keep review state here instead of `.tenet` in the project root, such as when the checkout
    /// is read-only.
    #[arg(
        long,
        global = true,
        env = "TENET_STATE_DIR",
        help_heading = "Global options"
    )]
    pub state_dir: Option<PathBuf>,
    /// Write a JSON Lines trace of spans and events to PATH, or to stderr with '-'.
    #[arg(
        long,
        global = true,
        env = "TENET_TRACE",
        value_name = "PATH",
        help_heading = "Diagnostics",
        hide_short_help = true
    )]
    pub trace: Option<PathBuf>,
    /// Environment that the trace's first record names; the default is ci when CI is set,
    /// otherwise local.
    #[arg(
        long,
        global = true,
        env = "TENET_TRACE_ENVIRONMENT",
        value_enum,
        default_value_t = Environment::detect(),
        hide_default_value = true,
        help_heading = "Diagnostics",
        hide_short_help = true
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

#[derive(Subcommand)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order, and ExitStatus follows its exit \
              codes."
)]
pub(crate) enum Command {
    /// Discover catalog recommendations, synthesize module contracts, and verify behavior checks.
    Scan(Scan),
    /// Initialize Tenet with recommended or starter contracts (see also `tenet scan`).
    Init(Init),
    /// Check provider credentials, sandbox tooling, runtimes, and repository setup.
    Doctor {
        /// Print diagnostic check results as a JSON document.
        #[arg(long)]
        json: bool,
    },
    /// Synthesize per-module contracts and falsify them with isolated behavior checks (see also
    /// `tenet scan`).
    Audit(Audit),
    /// Assess a repository, or whether a change preserves its contracts, and update the review.
    #[command(
        after_help = "Run tenet show CONTRACT to inspect a finding. Use --help for all check \
                      options."
    )]
    Check(Check),
    /// Apply safe native fixes for adopted contracts, then check the edited files.
    Fix(Fix),
    /// Show which contracts are settled and which are pending, without calling a model.
    Status {
        /// Report the diff review against this commit instead of the latest check's review.
        #[arg(long)]
        base: Option<String>,
        /// Compare a trusted local checker receipt with the current working tree.
        #[arg(long, value_name = "PATH")]
        evidence: Option<PathBuf>,
        /// Print the review status as a JSON document.
        #[arg(long)]
        json: bool,
        /// First remove cached answers this Tenet cannot reuse: those an earlier Tenet wrote and
        /// those a later answer to the same request replaced.
        #[arg(long)]
        prune_cache: bool,
    },
    /// Show what a reviewer needs to settle one contract: rule, change, model answer, and history.
    Show {
        /// Name of the contract to inspect.
        #[arg(required_unless_present = "pending")]
        contract: Option<String>,
        /// The selected paths of the check to show, when the contract stands in several.
        paths: Vec<PathBuf>,
        /// Show every pending contract, printing their shared change once.
        #[arg(long, conflicts_with = "contract")]
        pending: bool,
        /// Print the contract brief as a JSON document.
        #[arg(long)]
        json: bool,
    },
    /// Record a conclusion for a contract's current input, with the evidence that supports it.
    Record(Record),
    /// List checks and records, newest first.
    Log {
        /// Limit listed events to this contract name.
        contract: Option<String>,
        /// Print the matching review events as JSON Lines.
        #[arg(long)]
        json: bool,
    },
    /// Export captured contracts and source as JSON without calling a model.
    Export(Export),
    /// Validate contract documents without calling a model.
    Validate {
        /// Fail when an installed contract's text above its final `## Local adaptations` section
        /// differs from its release.
        #[arg(long)]
        strict_adaptations: bool,
    },
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
