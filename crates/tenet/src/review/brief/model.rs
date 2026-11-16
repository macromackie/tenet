use std::io::Write;

use tenet_engine::{Status, review::CurrentContract};

use super::super::{LISTED, SETTLED_STATUSES, check_command, format_tally};
use crate::{
    error::CommandError,
    report_format::{self, format_violation},
};

#[expect(
    clippy::too_many_lines,
    reason = "One saved assessment presents its combined and file coverage, then a scoped \
              follow-up."
)]
pub(super) fn write_model(
    out: &mut impl Write,
    entry: &CurrentContract<'_>,
) -> Result<(), CommandError> {
    let name = &entry.contract.name;
    if let Some(assessment) = entry.assessment {
        writeln!(out, "\nModel")?;
        match &assessment.conclusion {
            Some(conclusion) => {
                writeln!(
                    out,
                    "  {}{}: {}",
                    conclusion.status.as_str(),
                    format_violation(conclusion.violation),
                    conclusion.reason
                )?;
                if let Some(error) = &conclusion.error {
                    writeln!(out, "  {error}")?;
                }
                writeln!(
                    out,
                    "  {}",
                    report_format::format_combined(
                        conclusion.combined_status,
                        conclusion.context.as_ref()
                    )
                )?;
                if !conclusion.conflicting_files.is_empty() {
                    writeln!(
                        out,
                        "  Disagrees with file checks: {}",
                        conclusion.conflicting_files.join(", ")
                    )?;
                }
            },
            None => writeln!(out, "  not assessed")?,
        }
        let results = &assessment.results;
        if !results.is_empty() {
            let statuses = results
                .iter()
                .map(|result| report_format::label_status(result.status));
            writeln!(out, "  File checks: {}", format_tally(statuses))?;
        }
        let attention = |status: Status| !SETTLED_STATUSES.contains(&status);
        let mut ordered: Vec<_> = results.iter().collect();
        ordered.sort_by_key(|result| {
            (
                result.status != Status::Fail,
                !result.hunks.iter().any(|hunk| hunk.status == Status::Fail),
                result.path.as_str(),
            )
        });
        for result in ordered
            .iter()
            .filter(|result| results.len() <= LISTED || attention(result.status))
        {
            let omitted = report_format::format_omitted(&result.omitted);
            writeln!(
                out,
                "  {}  {}{}{}",
                result.path,
                report_format::label_status(result.status).to_lowercase(),
                format_violation(result.violation),
                if omitted.is_empty() {
                    omitted
                } else {
                    format!(" · {omitted}")
                }
            )?;
            if let Some(reason) = &result.reason {
                writeln!(out, "    {reason}")?;
            }
            if let Some(failure) = result.failure {
                writeln!(out, "    {}", report_format::request_failure(failure))?;
            }
            for line in report_format::format_hunks(&result.hunks) {
                writeln!(out, "    {line}")?;
            }
        }
        if let Some(result) = ordered.iter().find(|result| attention(result.status)) {
            let mut focused = entry.selection.clone();
            focused.min_confidence = entry.min_confidence;
            if entry.files.iter().any(|scoped| {
                scoped.file.path.to_string_lossy() == result.path && scoped.file.is_deleted()
            }) {
                focused.paths.clear();
            } else {
                focused.paths = vec![result.path.clone().into()];
            }
            writeln!(
                out,
                "\n  Focused check  {}",
                check_command(&focused, Some(name))
            )?;
            writeln!(
                out,
                "  Add related implementation with --context PATH:START-END. Inspect the \
                 command's selected scope."
            )?;
        }
    }

    Ok(())
}
