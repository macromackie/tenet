use serde::Serialize;

use super::{CheckResult, ContractResult, ContractStatus, Status};

/// Counts of a run's results by status, and its elapsed time.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Summary {
    pub passed: usize,
    pub failed: usize,
    pub uncertain: usize,
    pub not_applicable: usize,
    /// Execution errors, failed requests included.
    pub errors: usize,
    pub incomplete: usize,
    pub skipped: usize,
    pub requests: usize,
    /// Requests that returned no answer; their contracts stay pending.
    pub request_failures: usize,
    /// Failed requests that a rerun of the check may answer.
    pub retryable_failures: usize,
    /// Requests not sent because `--max-requests` was spent; their checks are incomplete.
    pub over_budget: usize,
    /// Request batches answered from the answer cache, and conclusions `check --no-model` reused,
    /// instead of the provider.
    pub cached: usize,
    pub attempts: usize,
    pub hedges: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// A contract's assessment time; a run's whole wall-clock time, which its caller measures.
    pub elapsed_ms: u128,
    pub cancelled: bool,
    pub contracts_verified: usize,
    pub contracts_clear: usize,
    pub contracts_preserved: usize,
    pub contracts_unaffected: usize,
    pub contracts_failed: usize,
    pub contracts_unresolved: usize,
}

impl Summary {
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Counts of results, contracts and requests are bounded by the repository's files \
                  and the request budget, far below the integer's limit."
    )]
    pub(crate) fn add(&mut self, result: &CheckResult) {
        self.record_request(result.request.as_ref());
        self.record_failure(result.failure);
        self.cached += usize::from(result.cached);
        match result.status {
            Status::Pass => self.passed += 1,
            Status::Fail => self.failed += 1,
            Status::Uncertain => self.uncertain += 1,
            Status::NotApplicable => self.not_applicable += 1,
            Status::Error => self.errors += 1,
            Status::Incomplete => self.incomplete += 1,
            Status::Skipped => self.skipped += 1,
        }
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Counts of results, contracts and requests are bounded by the repository's files \
                  and the request budget, far below the integer's limit."
    )]
    pub(crate) const fn merge(&mut self, other: &Self) {
        self.passed += other.passed;
        self.failed += other.failed;
        self.uncertain += other.uncertain;
        self.not_applicable += other.not_applicable;
        self.errors += other.errors;
        self.incomplete += other.incomplete;
        self.skipped += other.skipped;
        self.requests += other.requests;
        self.request_failures += other.request_failures;
        self.retryable_failures += other.retryable_failures;
        self.over_budget += other.over_budget;
        self.cached += other.cached;
        self.attempts += other.attempts;
        self.hedges += other.hedges;
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.contracts_verified += other.contracts_verified;
        self.contracts_clear += other.contracts_clear;
        self.contracts_preserved += other.contracts_preserved;
        self.contracts_unaffected += other.contracts_unaffected;
        self.contracts_failed += other.contracts_failed;
        self.contracts_unresolved += other.contracts_unresolved;
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Counts of results, contracts and requests are bounded by the repository's files \
                  and the request budget, far below the integer's limit."
    )]
    pub(crate) fn record_conclusion(&mut self, conclusion: &ContractResult) {
        self.record_request(conclusion.request.as_ref());
        self.record_failure(conclusion.failure);
        self.cached += usize::from(conclusion.cached);
        match conclusion.status {
            ContractStatus::Verified => self.contracts_verified += 1,
            ContractStatus::Clear => self.contracts_clear += 1,
            ContractStatus::Preserved => self.contracts_preserved += 1,
            ContractStatus::Unaffected => self.contracts_unaffected += 1,
            ContractStatus::Failed => self.contracts_failed += 1,
            ContractStatus::Unresolved => self.contracts_unresolved += 1,
        }
        if conclusion.error.is_some() {
            self.errors += 1;
        }
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Counts of results, contracts and requests are bounded by the repository's files \
                  and the request budget, far below the integer's limit."
    )]
    fn record_failure(&mut self, failure: Option<crate::RequestFailure>) {
        if let Some(failed) = failure {
            self.request_failures += 1;
            self.retryable_failures += usize::from(failed.retryable());
        }
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The request, attempt and token counts are bounded by the run's request and \
                  token budgets."
    )]
    const fn record_request(&mut self, request: Option<&ev_grep_core::RequestInfo>) {
        if let Some(info) = request {
            self.attempts += info.attempts;
            self.hedges += info.hedges;
            self.input_tokens += info.input_tokens;
            self.output_tokens += info.output_tokens;
        }
    }
}
