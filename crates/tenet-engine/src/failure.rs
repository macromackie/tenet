//! Why a model request returned no answer, and whether sending it again may succeed.

use ev_grep_core::Failure;
use serde::Serialize;

/// An evaluator error that names its failure. The provider's adapter knows its errors and its
/// retry table; the engine decides what each kind means for the review.
pub trait ClassifyRequest: Failure {
    /// Return the kind of failure the error reports, and whether another request may succeed.
    fn classify_request(&self) -> RequestFailure;
}

/// The kind of failure a model request met. The provider's adapter names it from the provider's
/// error; the names are stable in JSON output.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// HTTP 401, 402, or 403: the provider rejected the credential, or the key reached its limit,
    /// so every request would fail and the check stops.
    Authentication,
    /// HTTP 404, 405, a redirect, or an answer from another model: the endpoint or model does not
    /// exist, so every request would fail and the check stops.
    Configuration,
    /// No attempt received an HTTP response, or the connection failed during the exchange.
    Connection,
    /// HTTP 413, or a request larger than the provider's adapter sends.
    InputTooLarge,
    /// HTTP 499: the connection closed before the provider answered.
    Interrupted,
    /// Any other HTTP status the provider refused the request with.
    InvalidRequest,
    /// The provider's answer failed validation, or exceeded the size the adapter accepts.
    InvalidResponse,
    /// HTTP 403 with a body that names moderation reasons: only this request is refused.
    Moderation,
    /// HTTP 429.
    RateLimited,
    /// HTTP 5xx.
    ServerError,
    /// The request used its whole time budget, or the provider answered HTTP 408.
    Timeout,
    /// A failure the provider's adapter does not recognize.
    Unknown,
}

impl FailureKind {
    /// Return the failure kind's stable name, as JSON output spells it.
    #[must_use]
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Connection => "connection",
            Self::RateLimited => "rate_limited",
            Self::Interrupted => "interrupted",
            Self::ServerError => "server_error",
            Self::Authentication => "authentication",
            Self::Configuration => "configuration",
            Self::InputTooLarge => "input_too_large",
            Self::InvalidRequest => "invalid_request",
            Self::InvalidResponse => "invalid_response",
            Self::Moderation => "moderation",
            Self::Unknown => "unknown",
        }
    }
}

/// A failed model request: its kind, and whether rerunning the check unchanged may succeed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct RequestFailure {
    kind: FailureKind,
    /// Taken from the provider adapter's retry table.
    retryable: bool,
}

impl RequestFailure {
    /// Why the failure stops the check, when no request can succeed until the configuration
    /// changes.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The remaining variants take no action here, and listing each would repeat the \
                  enum."
    )]
    pub(crate) const fn explain_stop(self) -> Option<&'static str> {
        match self.kind {
            FailureKind::Authentication => {
                Some("the provider refused the request: credential rejected or key limit reached")
            },
            FailureKind::Configuration => {
                Some("the provider has no such endpoint or model; check --endpoint and --model")
            },
            _ => None,
        }
    }

    /// Return what kind of failure ended the request.
    #[must_use]
    #[inline]
    pub const fn kind(self) -> FailureKind {
        self.kind
    }

    /// Name a failure of `kind`, which another request may answer when `retryable`.
    #[must_use]
    #[inline]
    pub const fn new(kind: FailureKind, retryable: bool) -> Self {
        Self { kind, retryable }
    }

    /// Return whether a later request may succeed where this one failed.
    #[must_use]
    #[inline]
    pub const fn retryable(self) -> bool {
        self.retryable
    }
}
