use tenet_engine::{CheckResult, ConclusionReason, Event, Stage, Status};

use super::Reporter;

const REQUEST_STAGES: [Stage; 2] = [Stage::Applicability, Stage::Completeness];

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
    pub(crate) fn observe(&mut self, event: &Event) {
        if let Event::StageStarted { stage, .. } = event
            && REQUEST_STAGES.contains(stage)
        {
            self.summary.requests += 1;
        }
        if matches!(event, Event::FileBatchStarted { .. }) {
            self.summary.requests += 1;
        }
        if let Event::FileBatchCompleted {
            request, failure, ..
        } = event
        {
            self.track_request(request.as_ref());
            self.track_failure(*failure);
        }
        if let Event::HunkResult {
            cached,
            result,
            request,
            ..
        } = event
        {
            self.track_request(request.as_ref());
            self.track_failure(result.failure);
            self.summary.cached += usize::from(*cached);
            self.summary.execution_errors +=
                usize::from(result.status == Status::Error && result.failure.is_none());
        }
        if let Event::Result { result } = event {
            self.track_result(result);
        }
        if let Event::ContractFinished {
            conclusion: Some(conclusion),
            ..
        } = event
        {
            self.track_request(conclusion.request.as_ref());
            self.track_failure(conclusion.failure);
            self.summary.cached += usize::from(conclusion.cached);
            self.oversized_combined +=
                usize::from(conclusion.reason_code == ConclusionReason::ContextTooLarge);
        }
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Attempt and token counters are bounded by the run's request budget."
    )]
    fn track_failure(&mut self, failure: Option<tenet_engine::RequestFailure>) {
        if let Some(failed) = failure {
            self.summary.request_failures += 1;
            self.summary.retryable_failures += usize::from(failed.retryable());
        }
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Attempt and token counters are bounded by the run's request budget."
    )]
    const fn track_request(&mut self, request: Option<&ev_grep_core::RequestInfo>) {
        if let Some(info) = request {
            self.summary.attempts += info.attempts;
            self.summary.hedges += info.hedges;
            self.summary.input_tokens += info.input_tokens;
            self.summary.output_tokens += info.output_tokens;
        }
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Pair counters are bounded by the run's selected files and contracts."
    )]
    fn track_result(&mut self, result: &CheckResult) {
        if result.hunks.is_empty() {
            self.summary.cached += usize::from(result.cached);
            self.summary.execution_errors +=
                usize::from(result.status == Status::Error && result.failure.is_none());
        }
        if result.hunks.is_empty() && result.shared_request.is_none() {
            self.track_request(result.request.as_ref());
            self.track_failure(result.failure);
        }
        match result.status {
            Status::Pass => self.summary.passed += 1,
            Status::Fail => self.summary.failed += 1,
            Status::NotApplicable => self.summary.not_applicable += 1,
            Status::Uncertain => self.summary.uncertain += 1,
            Status::Error => self.summary.errors += 1,
            Status::Incomplete => {
                self.summary.incomplete += 1;
                if result
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("input was not truncated"))
                {
                    self.oversized_files += 1;
                }
            },
            Status::Skipped => self.summary.skipped += 1,
        }
    }
}
