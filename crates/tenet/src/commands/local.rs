use std::path::Path;

use serde_json::json;
use tenet_engine::mechanical::{self, Evidence, Freshness, LocalMode, Outcome};
use tokio::signal;

use crate::{cli::ExitStatus, error::CommandError, output, review::write_mechanical};

const PENDING: [Freshness; 2] = [Freshness::Missing, Freshness::Stale];

fn summarize_checks(rows: &[Evidence]) -> ExitStatus {
    if rows.is_empty() || rows.iter().any(|row| PENDING.contains(&row.freshness)) {
        return ExitStatus::Pending;
    }
    if rows.iter().any(|row| {
        row.freshness == Freshness::Current && row.observed_outcome == Some(Outcome::Fail)
    }) {
        return ExitStatus::Failed;
    }
    if rows.iter().all(|row| {
        row.freshness == Freshness::Current && row.observed_outcome == Some(Outcome::Pass)
    }) {
        return ExitStatus::Passed;
    }
    ExitStatus::Unresolved
}

pub(super) fn inspect(
    root: &Path,
    state: &Path,
    name: &str,
    json: bool,
) -> Result<ExitStatus, CommandError> {
    let rows =
        mechanical::inspect_local(root, state, Some(name).filter(|value| !value.is_empty()))?;
    render(&rows, json)
}

#[expect(
    clippy::integer_division_remainder_used,
    reason = "The select macro uses a remainder for branch order."
)]
pub(super) async fn run(
    root: &Path,
    state: &Path,
    name: &str,
    fresh: bool,
    json: bool,
) -> Result<ExitStatus, CommandError> {
    let mode = if fresh {
        LocalMode::Fresh
    } else {
        LocalMode::Reuse
    };
    let selection = Some(name).filter(|value| !value.is_empty());
    let rows = tokio::select! {
        biased;
        signal = signal::ctrl_c() => { signal?; return Ok(ExitStatus::Cancelled); },
        result = mechanical::run_local(root, state, selection, mode) => result?,
    };
    render(&rows, json)
}

fn render(rows: &[Evidence], json: bool) -> Result<ExitStatus, CommandError> {
    let result = summarize_checks(rows);
    let mut out = output::open_stdout().lock();
    if json {
        output::write_document(
            &mut out,
            1,
            json!({"checks": rows, "exit_code": u8::from(result)}),
        )?;
    } else if rows.is_empty() {
        use std::io::Write as _;
        writeln!(out, "No local checks declared.")?;
    } else {
        write_mechanical(&mut out, rows)?;
    }
    Ok(result)
}
