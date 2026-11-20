use std::path::PathBuf;

use clap::Subcommand;

#[derive(Subcommand)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order, and ExitStatus follows its exit \
              codes."
)]
pub(crate) enum Contracts {
    /// Suggest shared contracts for the public code in this repository, without installing them.
    Recommend {
        /// Read the highest release from this local catalog checkout instead of fetching it.
        #[arg(long, value_name = "DIR", env = "TENET_CATALOG_DIR")]
        library: Option<PathBuf>,
        /// Print recommendations and their supporting paths as JSON.
        #[arg(long)]
        json: bool,
    },
    /// List discovered contracts in this repository.
    List {
        /// List only the contracts that govern this file.
        #[arg(long = "for", value_name = "FILE")]
        path: Option<PathBuf>,
        /// Select scopes touched since this revision, including contract changes.
        #[arg(long)]
        base: Option<String>,
        /// Print discovered contracts and coverage as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Print one contract document by name.
    Show {
        /// Name of the contract to display.
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
        /// Explicit 3-digit folder prefix instead of the next free number.
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
        /// Print comparison results as JSON.
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
        /// Print update outcomes as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show upstream changes and local edits since the locked version.
    Diff {
        /// Path to the installed contract or skill copy.
        path: PathBuf,
        /// Print the base, upstream, and local texts.
        #[arg(long)]
        json: bool,
    },
    /// Remove an installed copy and its lock entry.
    Remove {
        /// Path to the installed contract or skill copy.
        path: PathBuf,
        /// Keep the files as ordinary local files.
        #[arg(long)]
        keep_file: bool,
    },
}
