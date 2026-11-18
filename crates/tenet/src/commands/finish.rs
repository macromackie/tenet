use std::time::Instant;

use tenet_engine::{Coverage, Event, review::Checked};

use super::final_review::FinalReview;
use crate::{cli::ExitStatus, error::CommandError};

#[expect(
    clippy::future_not_send,
    reason = "The CLI drives the scoped review and reporter on one thread."
)]
pub(super) async fn finish(
    final_review: Option<&FinalReview<'_>>,
    emit: &impl Fn(Event) -> Result<(), CommandError>,
    checked: &Checked,
    revision: Option<u64>,
    coverage: Option<Coverage>,
    started: Instant,
) -> Result<ExitStatus, CommandError> {
    if let Some(review) = final_review {
        return review.complete(checked, revision).await;
    }
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
