//! Why a model request returned no answer, and whether sending it again may succeed.

use serde::Serialize;

/// The kind of failure a model request met. The provider's adapter names it from the provider's
/// error; the names are stable in JSON output.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// The request used its whole time budget, or the provider answered HTTP 408.
    Timeout,
    /// No attempt received an HTTP response.
    Connection,
    /// HTTP 429.
    RateLimited,
    /// HTTP 499: the connection closed before the provider answered.
    Interrupted,
    /// HTTP 5xx.
    ServerError,
    /// HTTP 401 or 402: the provider rejected the credential or its account, so every request
    /// would fail and the check stops.
    Authentication,
    /// HTTP 404, 405, or a redirect: the endpoint or model does not exist, so every request would
    /// fail and the check stops.
    Configuration,
    /// HTTP 413, or a request larger than the provider's adapter sends.
    InputTooLarge,
    /// Any other HTTP status the provider refused the request with.
    InvalidRequest,
    /// A failure the provider's adapter does not recognize.
    Unknown,
}

impl FailureKind {
    pub fn as_str(self) -> &'static str {
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
            Self::Unknown => "unknown",
        }
    }
}

/// A failed model request: its kind, and whether rerunning the check unchanged may succeed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct RequestFailure {
    kind: FailureKind,
    /// Derived from `kind`, so the two never disagree.
    retryable: bool,
}

impl From<FailureKind> for RequestFailure {
    fn from(kind: FailureKind) -> Self {
        let retryable = matches!(
            kind,
            FailureKind::Timeout
                | FailureKind::Connection
                | FailureKind::RateLimited
                | FailureKind::Interrupted
                | FailureKind::ServerError
        );
        Self { kind, retryable }
    }
}

impl RequestFailure {
    pub fn kind(self) -> FailureKind {
        self.kind
    }

    pub fn retryable(self) -> bool {
        self.retryable
    }

    /// Why the failure stops the check, when no request can succeed until the configuration
    /// changes.
    pub(crate) fn stops_check(self) -> Option<&'static str> {
        match self.kind {
            FailureKind::Authentication => Some("the provider rejected the credentials"),
            FailureKind::Configuration => {
                Some("the provider has no such endpoint or model; check --endpoint and --model")
            }
            _ => None,
        }
    }
}
