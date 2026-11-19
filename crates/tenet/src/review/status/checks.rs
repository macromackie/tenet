use std::io;

use serde_json::json;
use tenet_engine::mechanical;

use crate::{cli::ExitStatus, error::CommandError, output, review::STATUS_VERSION};

pub(crate) fn write_mechanical(
    out: &mut impl io::Write,
    rows: &[mechanical::Evidence],
) -> Result<(), CommandError> {
    if rows.is_empty() {
        return Ok(());
    }
    writeln!(out, "\n Local checks")?;
    for row in rows {
        write!(
            out,
            " {}/{}  {:?}",
            row.contract,
            row.name.as_str(),
            row.freshness
        )?;
        if let Some(outcome) = row.observed_outcome {
            write!(out, " · observed {outcome:?}")?;
        }
        if let Some(checker) = &row.checker {
            write!(out, " by {checker:?}")?;
        }
        if row.replay.is_none()
            && let Some(message) = &row.message
        {
            write!(out, " · {message:?}")?;
        }
        if let Some(reason) = &row.reason {
            write!(out, " · {reason}")?;
        }
        if row.reused == Some(true) {
            write!(out, " (reused)")?;
        }
        writeln!(out)?;
        if let Some(replay) = &row.replay {
            write_details(out, row)?;
            writeln!(out, "   Replay    {replay}")?;
        }
    }
    Ok(())
}

fn write_details(out: &mut impl io::Write, row: &mechanical::Evidence) -> io::Result<()> {
    if let Some(message) = &row.message {
        for line in message.lines() {
            writeln!(out, "   {}", line.escape_debug())?;
        }
    }
    if let Some(finding) = &row.finding {
        writeln!(out, "   Case      {}", finding.case.escape_debug())?;
        writeln!(out, "   Expected  {}", finding.expected.escape_debug())?;
        writeln!(out, "   Actual    {}", finding.actual.escape_debug())?;
        if let Some(location) = &finding.location {
            writeln!(out, "   Location  {}", location.escape_debug())?;
        }
    }
    Ok(())
}

#[expect(
    clippy::indexing_slicing,
    reason = "The document is built as an object before adding optional mechanical evidence."
)]
pub(super) fn write_unchecked(
    out: &mut impl io::Write,
    base: Option<&str>,
    message: &str,
    mechanical: Option<&[mechanical::Evidence]>,
    json: bool,
) -> Result<ExitStatus, CommandError> {
    if json {
        let mut document = json!({
            "base": base,
            "revision": null,
            "contracts": [],
            "exit_code": u8::from(ExitStatus::Pending),
            "message": message,
        });
        if let Some(rows) = mechanical {
            document["mechanical_evidence"] = json!(rows);
        }
        output::write_document(out, STATUS_VERSION, document)?;
    } else {
        writeln!(out, "{message}")?;
        if let Some(rows) = mechanical {
            write_mechanical(out, rows)?;
        }
    }
    Ok(ExitStatus::Pending)
}
