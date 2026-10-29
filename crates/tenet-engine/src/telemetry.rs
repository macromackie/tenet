//! The engine's trace records: the `tenet.contract`, `tenet.pair` and `tenet.wait` spans its
//! check opens, and the events below. A binary that installs a subscriber writes them.
//!
//! Records hold kinds, counts and identifiers, never a contract's text, a prompt, or file contents.

/// The events the engine records. Each event names its member in an `event` field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LogEvent {
    /// The request budget refused a request.
    BudgetExhausted,
    /// An earlier check's answers stood in for a request.
    CacheHit,
}

impl LogEvent {
    const fn name(self) -> &'static str {
        match self {
            Self::BudgetExhausted => "budget.exhausted",
            Self::CacheHit => "cache.hit",
        }
    }
}

/// Which request an event concerns: one file's, inside its `tenet.pair` span, or a contract's
/// combined assessment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Request {
    Combined,
    File,
}

impl Request {
    const fn name(self) -> &'static str {
        match self {
            Self::Combined => "combined",
            Self::File => "file",
        }
    }
}

/// Record that the request budget of `max_requests` refused `request`.
pub(crate) fn record_budget_exhausted(request: Request, max_requests: usize) {
    tracing::info!(
        event = LogEvent::BudgetExhausted.name(),
        request = request.name(),
        max_requests,
    );
}

/// Record that cached answers stood in for `request`.
pub(crate) fn record_cache_hit(request: Request) {
    tracing::info!(event = LogEvent::CacheHit.name(), request = request.name());
}
