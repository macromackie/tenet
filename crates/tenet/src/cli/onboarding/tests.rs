use anyhow::Result;
use clap::Parser as _;

use super::SeverityArg;
use crate::cli::{Cli, Command};

#[test]
fn parse_audit_default_deep_shallow_and_conflicts() -> Result<()> {
    let default_parsed = Cli::try_parse_from(["tenet", "audit"])?;
    let Command::Audit(default_args) = default_parsed.command else {
        anyhow::bail!("expected audit subcommand");
    };
    anyhow::ensure!(
        !default_args.deep && !default_args.shallow && default_args.is_deep(),
        "expected audit to default to deep mode"
    );

    let shallow_parsed = Cli::try_parse_from(["tenet", "audit", "--shallow"])?;
    let Command::Audit(shallow_args) = shallow_parsed.command else {
        anyhow::bail!("expected audit subcommand");
    };
    anyhow::ensure!(
        shallow_args.shallow && !shallow_args.is_deep(),
        "expected audit --shallow to disable deep mode"
    );

    let deep_parsed = Cli::try_parse_from(["tenet", "audit", "--deep"])?;
    let Command::Audit(deep_args) = deep_parsed.command else {
        anyhow::bail!("expected audit subcommand");
    };
    anyhow::ensure!(
        deep_args.deep && !deep_args.shallow && deep_args.is_deep(),
        "expected audit --deep to remain accepted in deep mode"
    );

    let conflict = Cli::try_parse_from(["tenet", "audit", "--deep", "--shallow"]);
    anyhow::ensure!(
        conflict.is_err(),
        "expected audit --deep to conflict with --shallow"
    );
    Ok(())
}

#[test]
fn parse_audit_replay_flag_and_conflicts() -> Result<()> {
    let parsed = Cli::try_parse_from(["tenet", "audit", "--replay", "src/app.py"])?;
    let Command::Audit(args) = parsed.command else {
        anyhow::bail!("expected audit subcommand");
    };
    anyhow::ensure!(args.replay, "expected --replay to be set");
    anyhow::ensure!(args.paths.len() == 1);

    for conflicting in ["--fresh", "--deep", "--shallow", "--save"] {
        let result = Cli::try_parse_from(["tenet", "audit", "--replay", conflicting]);
        anyhow::ensure!(
            result.is_err(),
            "expected --replay to conflict with {conflicting:?}"
        );
    }
    Ok(())
}

#[test]
fn parse_check_skip_semantic_and_aliases() -> Result<()> {
    let skip_semantic = Cli::try_parse_from(["tenet", "check", "--skip-semantic", "--json"])?;
    let Command::Check(args) = skip_semantic.command else {
        anyhow::bail!("expected check subcommand");
    };
    anyhow::ensure!(args.skip_semantic, "expected --skip-semantic to be set");
    anyhow::ensure!(args.run.json, "expected --json to be set");

    let offline = Cli::try_parse_from(["tenet", "check", "--offline"])?;
    let Command::Check(offline_args) = offline.command else {
        anyhow::bail!("expected check subcommand");
    };
    anyhow::ensure!(
        offline_args.skip_semantic,
        "expected --offline alias to set skip_semantic"
    );

    let replay = Cli::try_parse_from(["tenet", "check", "--replay"])?;
    let Command::Check(replay_args) = replay.command else {
        anyhow::bail!("expected check subcommand");
    };
    anyhow::ensure!(
        replay_args.skip_semantic,
        "expected --replay alias to set skip_semantic"
    );
    Ok(())
}

#[test]
fn parse_fix_flags_severity_and_conflicts() -> Result<()> {
    let parsed = Cli::try_parse_from([
        "tenet",
        "fix",
        "--from-audit",
        "--max-requests",
        "10",
        "--severity",
        "high",
        "--contract",
        "demo-rule",
        "src/app.py",
    ])?;
    let Command::Fix(args) = parsed.command else {
        anyhow::bail!("expected fix subcommand");
    };
    anyhow::ensure!(args.from_audit, "expected --from-audit to be set");
    anyhow::ensure!(args.max_requests == 10, "expected max_requests == 10");
    anyhow::ensure!(args.severity == SeverityArg::High);
    anyhow::ensure!(args.contract.as_deref() == Some("demo-rule"));
    anyhow::ensure!(args.paths.len() == 1);

    let unified = Cli::try_parse_from([
        "tenet",
        "fix",
        "--max-requests",
        "5",
        "--severity",
        "critical",
    ])?;
    let Command::Fix(unified_args) = unified.command else {
        anyhow::bail!("expected fix subcommand");
    };
    anyhow::ensure!(!unified_args.from_audit);
    anyhow::ensure!(unified_args.max_requests == 5);
    anyhow::ensure!(unified_args.severity == SeverityArg::Critical);

    let default_fix = Cli::try_parse_from(["tenet", "fix"])?;
    let Command::Fix(default_args) = default_fix.command else {
        anyhow::bail!("expected fix subcommand");
    };
    anyhow::ensure!(default_args.max_requests == 20);
    anyhow::ensure!(default_args.severity == SeverityArg::Medium);

    let conflict = Cli::try_parse_from(["tenet", "fix", "--from-audit", "--base", "HEAD"]);
    anyhow::ensure!(
        conflict.is_err(),
        "expected --from-audit to conflict with --base"
    );
    Ok(())
}

#[test]
fn parse_scan_default_deep_shallow_and_conflicts() -> Result<()> {
    let default_parsed = Cli::try_parse_from(["tenet", "scan"])?;
    let Command::Scan(default_args) = default_parsed.command else {
        anyhow::bail!("expected scan subcommand");
    };
    anyhow::ensure!(
        !default_args.deep && !default_args.shallow && default_args.is_deep(),
        "expected scan to default to deep mode"
    );

    let shallow_parsed = Cli::try_parse_from(["tenet", "scan", "--shallow"])?;
    let Command::Scan(shallow_args) = shallow_parsed.command else {
        anyhow::bail!("expected scan subcommand");
    };
    anyhow::ensure!(
        shallow_args.shallow && !shallow_args.is_deep(),
        "expected scan --shallow to disable deep mode"
    );

    let deep_parsed = Cli::try_parse_from(["tenet", "scan", "--deep"])?;
    let Command::Scan(deep_args) = deep_parsed.command else {
        anyhow::bail!("expected scan subcommand");
    };
    anyhow::ensure!(
        deep_args.deep && !deep_args.shallow && deep_args.is_deep(),
        "expected scan --deep to remain accepted in deep mode"
    );

    let conflict = Cli::try_parse_from(["tenet", "scan", "--deep", "--shallow"]);
    anyhow::ensure!(
        conflict.is_err(),
        "expected scan --deep to conflict with --shallow"
    );
    Ok(())
}

#[test]
fn parse_scan_flags_aliases_and_conflicts() -> Result<()> {
    let applied = Cli::try_parse_from(["tenet", "scan", "--yes", "--json"])?;
    let Command::Scan(args) = applied.command else {
        anyhow::bail!("expected scan subcommand");
    };
    anyhow::ensure!(args.apply, "expected --yes alias to set apply");
    anyhow::ensure!(args.json);

    let starter_default = Cli::try_parse_from(["tenet", "scan", "--starter"])?;
    let Command::Scan(starter_args) = starter_default.command else {
        anyhow::bail!("expected scan subcommand");
    };
    anyhow::ensure!(starter_args.starter == Some(None));

    let starter_named = Cli::try_parse_from(["tenet", "scan", "--starter", "api-guards"])?;
    let Command::Scan(named_args) = starter_named.command else {
        anyhow::bail!("expected scan subcommand");
    };
    anyhow::ensure!(named_args.starter == Some(Some(String::from("api-guards"))));

    let conflict = Cli::try_parse_from(["tenet", "scan", "--apply", "--ephemeral"]);
    anyhow::ensure!(
        conflict.is_err(),
        "expected --apply to conflict with --ephemeral"
    );
    Ok(())
}
