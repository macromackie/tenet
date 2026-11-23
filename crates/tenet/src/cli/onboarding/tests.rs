use anyhow::Result;
use clap::Parser as _;

use crate::cli::{Cli, Command};

#[test]
fn parse_audit_replay_flag_and_conflicts() -> Result<()> {
    let parsed = Cli::try_parse_from(["tenet", "audit", "--replay", "src/app.py"])?;
    let Command::Audit(args) = parsed.command else {
        anyhow::bail!("expected audit subcommand");
    };
    anyhow::ensure!(args.replay, "expected --replay to be set");
    anyhow::ensure!(args.paths.len() == 1);

    for conflicting in ["--fresh", "--deep", "--save"] {
        let result = Cli::try_parse_from(["tenet", "audit", "--replay", conflicting]);
        anyhow::ensure!(
            result.is_err(),
            "expected --replay to conflict with {conflicting:?}"
        );
    }
    Ok(())
}

#[test]
fn parse_fix_from_audit_flags_and_conflicts() -> Result<()> {
    let parsed = Cli::try_parse_from([
        "tenet",
        "fix",
        "--from-audit",
        "--max-requests",
        "10",
        "--contract",
        "demo-rule",
        "src/app.py",
    ])?;
    let Command::Fix(args) = parsed.command else {
        anyhow::bail!("expected fix subcommand");
    };
    anyhow::ensure!(args.from_audit, "expected --from-audit to be set");
    anyhow::ensure!(args.max_requests == 10, "expected max_requests == 10");
    anyhow::ensure!(args.contract.as_deref() == Some("demo-rule"));
    anyhow::ensure!(args.paths.len() == 1);

    let default_budget = Cli::try_parse_from(["tenet", "fix", "--from-audit"])?;
    let Command::Fix(default_args) = default_budget.command else {
        anyhow::bail!("expected fix subcommand");
    };
    anyhow::ensure!(
        default_args.max_requests == 20,
        "expected default max_requests 20"
    );

    let conflict = Cli::try_parse_from(["tenet", "fix", "--from-audit", "--base", "HEAD"]);
    anyhow::ensure!(
        conflict.is_err(),
        "expected --from-audit to conflict with --base"
    );

    let missing_flag = Cli::try_parse_from(["tenet", "fix", "--max-requests", "5"]);
    anyhow::ensure!(
        missing_flag.is_err(),
        "expected --max-requests to require --from-audit"
    );
    Ok(())
}
