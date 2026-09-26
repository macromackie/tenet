use super::*;
use crate::{
    cli::{Cli, Command},
    fixture_report::Totals,
    fixture_run::assess,
};
use clap::Parser;
use ev_grep_core::{Assessment, Evaluator, Outcome, Source};

struct Model;
impl Evaluator for Model {
    async fn assess(&self, query: &str, source: &Source) -> Result<Assessment> {
        ensure!(
            !query.contains("LABEL_SENTINEL") && !source.text.contains("LABEL_SENTINEL"),
            "expected answer leaked"
        );
        ensure!(
            !source.text.contains("eval.toml"),
            "manifest entered model inputs"
        );
        Ok(Assessment {
            outcome: Outcome::Match,
            choice: Outcome::Match,
            confidence: 1.0,
            reason: None,
            probabilities: BTreeMap::new(),
            model: "fixture".into(),
            input_tokens: 1,
            output_tokens: 1,
        })
    }
}

#[tokio::test]
async fn repository_cases_score_real_checks_without_sending_expectations() -> Result<()> {
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("base"))?;
    fs::create_dir_all(root.path().join(".contracts/001-errors"))?;
    fs::write(root.path().join("base/code.ts"), "catch { return []; }\n")?;
    fs::write(
        root.path().join(".contracts/001-errors/CONTRACT.md"),
        "---\nname: errors\nmessage: Keep failures.\n---\n## Rules\nPreserve database failures.\n",
    )?;
    fs::write(
        root.path().join("eval.toml"),
        "[[cases]]\nname = 'LABEL_SENTINEL'\n[cases.expect]\nerrors = 'failed'\n\n[[cases]]\nname = 'mismatch'\n[cases.expect]\nerrors = 'verified'\n",
    )?;
    let cli = Cli::try_parse_from(["tenet", "eval", root.path().to_str().context("path")?])?;
    let Command::Eval(args) = cli.command else {
        anyhow::bail!("expected eval");
    };
    let cases = discover(root.path(), &args)?;
    let options = tenet_engine::Options {
        jobs: 1,
        max_requests: 10,
    };
    let mut totals = Totals::default();
    for case in &cases {
        let prepared = case.prepare()?;
        let result = assess(case, &prepared, options, &Model, &|_| Ok(())).await?;
        totals.add(&result);
    }
    assert_eq!((totals.matched, totals.mismatched), (1, 1));
    assert_eq!((totals.exit_code(false), totals.exit_code(true)), (0, 1));
    let prepared = cases[0].prepare()?;
    let limited = assess(
        &cases[0],
        &prepared,
        tenet_engine::Options {
            max_requests: 1,
            ..options
        },
        &Model,
        &|_| Ok(()),
    )
    .await?;
    totals.add(&limited);
    assert_eq!(totals.exit_code(false), 2);
    assert_eq!(
        fs::read_to_string(root.path().join("base/code.ts"))?,
        "catch { return []; }\n"
    );
    fs::write(
        root.path().join("eval.toml"),
        "[[cases]]\nname = 'bad'\n[cases.expect]\nmissing = 'verified'\n",
    )?;
    assert!(discover(root.path(), &args)?[0].prepare().is_err());
    Ok(())
}
