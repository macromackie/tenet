use std::{io::Write as _, path::Path};

use serde_json::json;
use tenet_engine::mechanical::{self, Freshness, Outcome};
use tokio::signal;

use crate::{cli::ExitStatus, error::CommandError, output};

#[expect(
    clippy::integer_division_remainder_used,
    reason = "The select macro uses a remainder for branch order."
)]
pub(super) async fn run(
    root: &Path,
    selector: &str,
    json: bool,
) -> Result<ExitStatus, CommandError> {
    let report = tokio::select! {
        biased;
        signal = signal::ctrl_c() => { signal?; return Ok(ExitStatus::Cancelled); },
        result = mechanical::qualify(root, selector) => result?,
    };
    let result = match (report.freshness, report.outcome) {
        (Freshness::Missing | Freshness::Stale, _) => ExitStatus::Pending,
        (Freshness::Current, Outcome::Pass) => ExitStatus::Passed,
        (Freshness::Current, Outcome::Fail) => ExitStatus::Failed,
        _ => ExitStatus::Unresolved,
    };
    let mut out = output::open_stdout().lock();
    if json {
        output::write_document(
            &mut out,
            1,
            json!({
                "cases": report.cases, "freshness": report.freshness, "outcome": report.outcome,
                "binding_sha256":report.binding_sha256, "contract_sha256":report.contract_sha256,
                "checker_sha256":report.checker_sha256, "manifest_sha256":report.manifest_sha256,
                "reason": report.reason, "selector": selector, "exit_code": u8::from(result)
            }),
        )?;
    } else {
        writeln!(
            out,
            "Checker {selector}: {:?} ({:?})",
            report.outcome, report.freshness
        )?;
        if let Some(reason) = &report.reason {
            writeln!(out, "{reason}")?;
        }
        for case in &report.cases {
            writeln!(
                out,
                "{} ({}) {:?}: expected {:?}, observed {:?}. {}",
                case.name, case.kind, case.outcome, case.expected, case.observed, case.message
            )?;
        }
    }
    Ok(result)
}
