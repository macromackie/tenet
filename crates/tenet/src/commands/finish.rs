use std::time::Instant;

use tenet_engine::{Coverage, Event, review::Checked};

use crate::{cli::ExitStatus, error::CommandError};

pub(super) fn finish(
    emit: &impl Fn(Event) -> Result<(), CommandError>,
    checked: &Checked,
    revision: Option<u64>,
    coverage: Option<Coverage>,
    started: Instant,
) -> Result<ExitStatus, CommandError> {
    let status = ExitStatus::from(checked.summarize_overall());
    let pending = checked.list_pending();
    let mut summary = checked.summary;
    summary.elapsed_ms = started.elapsed().as_millis();
    emit(Event::Summary {
        summary,
        revision,
        pending,
        coverage,
        exit_code: u8::from(status),
    })?;
    Ok(status)
}
