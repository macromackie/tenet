use super::Reporter;
use tenet_engine::{CheckResult, Event, Stage, Status};

#[derive(Default)]
pub(super) struct ContractProgress {
    pub(super) total: usize,
    pub(super) done: usize,
    pub(super) completing: bool,
    pub(super) issues: Vec<CheckResult>,
}

impl Reporter<'_> {
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
            match result.status {
                Status::Pass => self.summary.passed += 1,
                Status::Fail => self.summary.failed += 1,
                Status::NotApplicable => self.summary.not_applicable += 1,
                Status::Uncertain => self.summary.uncertain += 1,
                Status::Error => self.summary.errors += 1,
                Status::Incomplete => self.summary.incomplete += 1,
            }
        }
    }
}
