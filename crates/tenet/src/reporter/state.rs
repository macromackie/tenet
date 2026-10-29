use tenet_engine::{CheckResult, Event, Stage, Status};

use super::Reporter;

#[derive(Default)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Fields follow the order a contract's progress fills them."
)]
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "The sibling modules fill these fields, and the type stays private to its parent \
              module."
)]
pub(super) struct ContractProgress {
    pub(super) total: usize,
    pub(super) done: usize,
    pub(super) completing: bool,
    pub(super) issues: Vec<CheckResult>,
}

impl Reporter<'_> {
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Counters are bounded by the requests and contracts of one run."
    )]
    pub(super) fn track(&mut self, event: &Event) {
        if matches!(
            event,
            Event::StageStarted {
                stage: Stage::Applicability | Stage::Completeness,
                ..
            }
        ) {
            self.summary.requests += 1;
        }
        if let Event::Result { result } = event {
            if let Some(failure) = result.failure {
                self.summary.request_failures += 1;
                self.summary.retryable_failures += usize::from(failure.retryable());
            }
            match result.status {
                Status::Pass => self.summary.passed += 1,
                Status::Fail => self.summary.failed += 1,
                Status::NotApplicable => self.summary.not_applicable += 1,
                Status::Uncertain => self.summary.uncertain += 1,
                Status::Error => self.summary.errors += 1,
                Status::Incomplete => self.summary.incomplete += 1,
                Status::Skipped => self.summary.skipped += 1,
            }
        }
    }
}
