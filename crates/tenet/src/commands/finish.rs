use core::cell::RefCell;
use std::{path::Path, time::Instant};

use tenet_engine::{Event, Plan, review::Checked};

use super::final_review::FinalReview;
use crate::{
    cli::{Check, ExitStatus},
    error::CommandError,
    generated,
    reporter::Reporter,
};

#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "The command prepares a scoped review and passes it to the reporter."
)]
pub(super) struct Completion<'run, 'context> {
    pub(super) check: &'run Check,
    pub(super) final_review: Option<&'run FinalReview<'run>>,
    pub(super) plan: &'run Plan,
    pub(super) reporter: &'run RefCell<Reporter<'context>>,
    pub(super) started: Instant,
    pub(super) state: &'run Path,
}

impl Completion<'_, '_> {
    #[expect(
        clippy::future_not_send,
        reason = "The CLI drives the scoped review and reporter on one thread."
    )]
    pub(super) async fn finish(
        &self,
        emit: &impl Fn(Event) -> Result<(), CommandError>,
        checked: &Checked,
        revision: Option<u64>,
    ) -> Result<ExitStatus, CommandError> {
        if let Some(review) = self.final_review {
            return review.complete(checked, revision).await;
        }
        let live = self.reporter.borrow().is_live();
        let mut report_phase = |phase: &str| {
            drop(self.reporter.borrow_mut().refresh_behavior(phase));
        };
        let on_progress: Option<&mut dyn FnMut(&str)> = live.then_some(&mut report_phase);
        let rows =
            generated::assess(self.plan, self.state, self.check, checked, on_progress).await?;
        self.reporter.borrow_mut().clear_progress()?;
        self.reporter.borrow_mut().write_generated(&rows, checked)?;
        let status = ExitStatus::from(
            checked
                .summarize_overall()
                .max(generated::summarize_status(&rows)),
        );
        let mut summary = checked.summary;
        summary.elapsed_ms = self.started.elapsed().as_millis();
        emit(Event::Summary {
            summary,
            revision,
            pending: checked.list_pending(),
            coverage: self.plan.coverage.clone(),
            exit_code: u8::from(status),
        })?;
        Ok(status)
    }
}
