use std::{
    io::{self, Write as _},
    path::Path,
};

use tenet_engine::{Selection, fixes};

use super::prepare::resolve_pr_base;
use crate::{
    cli::{ExitStatus, Fix},
    error::CommandError,
    output,
};

pub(super) async fn run(root: &Path, args: &Fix) -> Result<ExitStatus, CommandError> {
    let base = args.base.as_deref();
    let contract = args.contract.as_deref();
    let json = args.json;
    let selection = Selection {
        base: match base {
            Some(reference) => Some(reference.to_owned()),
            None => resolve_pr_base(root)?,
        },
        contract: contract.map(str::to_owned),
        paths: args.paths.clone(),
    };
    let plan = tenet_engine::plan(root, &selection)?;
    let report = fixes::apply(&plan).await.map_err(anyhow::Error::from)?;
    let status = if !report.unresolved.is_empty() {
        ExitStatus::Unresolved
    } else if report.verified.iter().any(|check| !check.passed) {
        ExitStatus::Failed
    } else {
        ExitStatus::Passed
    };
    if json {
        let document = serde_json::json!({
            "changes": report.changes, "manual": report.manual,
            "checks": report.verified, "unresolved": report.unresolved,
            "exit_code": u8::from(status)
        });
        output::write_document(&mut output::open_stdout(), 1, document)?;
    } else {
        render(&report)?;
    }
    Ok(status)
}

fn render(report: &fixes::Report) -> io::Result<()> {
    let mut result = output::open_stdout().lock();
    for change in &report.changes {
        writeln!(result, "{}", change.patch)?;
    }
    for row in &report.unresolved {
        writeln!(
            result,
            "Unresolved  {} ({}) — {}",
            row.contract, row.tool, row.message
        )?;
    }
    if !report.unresolved.is_empty() {
        writeln!(result, "No fixes applied.")?;
    } else if report.verified.is_empty() {
        writeln!(
            result,
            "No safe native fixer applies to the selected files."
        )?;
    } else {
        writeln!(result, "{} files updated.", report.changes.len())?;
        for check in &report.verified {
            let observation = if check.passed {
                "Passed"
            } else {
                "Findings remain"
            };
            writeln!(result, "{observation}  {} ({})", check.contract, check.tool)?;
            if let Some(finding) = &check.finding {
                writeln!(result, "  {finding}")?;
            }
        }
    }
    if !report.manual.is_empty() {
        writeln!(result, "No native fix for {}.", report.manual.join(", "))?;
    }
    writeln!(result, "Run tenet check to review the resulting code.")
}
