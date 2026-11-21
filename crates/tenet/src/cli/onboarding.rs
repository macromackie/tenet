use std::path::PathBuf;

use clap::Args;

#[derive(Args)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order."
)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "These are independent user-facing CLI flags parsed by clap."
)]
pub(crate) struct Audit {
    /// Files or directories to audit; discovers production modules when omitted.
    pub paths: Vec<PathBuf>,
    /// Synthesize adversarial edge-case contracts in addition to API and doc guarantees.
    #[arg(long)]
    pub deep: bool,
    /// Save contracts with verified checks into `.contracts/`.
    #[arg(long)]
    pub save: bool,
    /// Re-audit selected modules instead of resuming from `.tenet/audit.json`.
    #[arg(long)]
    pub fresh: bool,
    /// Maximum model requests across synthesis and behavior checks.
    #[arg(long, default_value_t = 120)]
    pub max_requests: u32,
    /// Print the audit report as a JSON document.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order."
)]
pub(crate) struct Init {
    /// Read recommendations from this local catalog checkout instead of fetching the public
    /// catalog.
    #[arg(long, value_name = "DIR", env = "TENET_CATALOG_DIR")]
    pub library: Option<PathBuf>,
    /// Directory whose .contracts receives newly created or installed contracts.
    #[arg(long, default_value = ".")]
    pub scope: PathBuf,
    /// Scaffold a local starter CONTRACT.md instead of querying the catalog (optional kebab-case
    /// name).
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "project-invariants",
        value_name = "NAME",
        conflicts_with = "library"
    )]
    pub starter: Option<String>,
    /// Install recommended contracts immediately instead of only printing commands.
    #[arg(long)]
    pub yes: bool,
    /// Print initialization recommendations and created files as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub(crate) struct Fix {
    /// Limit fixes to files changed since this commit.
    #[arg(long)]
    pub base: Option<String>,
    /// Apply fixes for one adopted contract.
    #[arg(long)]
    pub contract: Option<String>,
    /// Print applied fixes and re-check results as JSON.
    #[arg(long)]
    pub json: bool,
    /// Files or directories to fix; fixes all governed files when omitted.
    pub paths: Vec<PathBuf>,
}
