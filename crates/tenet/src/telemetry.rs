//! The trace that `--trace` writes: one JSON record per line, holding Tenet's and Jev's spans and
//! events.
//!
//! Its first record names the trace schema, the binary's version and revision, and the
//! environment. Records hold statuses, kinds, counts, identifiers, and durations, never a
//! contract, a prompt, file contents, a credential, a header, or a provider message.

use alloc::sync::Arc;
use std::{env, fs::File, path::Path};

use clap::ValueEnum;
use tenet_engine::{CheckError, FailureKind, RequestFailure};
use tracing::{Level, Span, field};
use tracing_subscriber::{
    filter::Targets,
    fmt::{self, format::FmtSpan, writer::BoxMakeWriter},
    prelude::*,
};

use crate::{error::CommandError, output};

/// The version of the trace's record shapes. Raise it when a record's fields change meaning or a
/// field is removed.
const TRACE_SCHEMA_VERSION: u32 = 2;

/// The events Tenet records. Each event names its member in an `event` field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LogEvent {
    /// A failure every later request would meet ended the check before it concluded.
    CheckStopped,
    /// A file's request or a combined assessment returned no answer.
    RequestFailed,
    /// The provider's shared rate limit prevented the request from obtaining an answer.
    RequestRateLimited,
    /// The trace began; this is its first record.
    TraceStarted,
}

impl LogEvent {
    const fn name(self) -> &'static str {
        match self {
            Self::CheckStopped => "check.stopped",
            Self::RequestFailed => "request.failed",
            Self::RequestRateLimited => "request.rate_limited",
            Self::TraceStarted => "trace.started",
        }
    }
}

/// Where a trace was recorded, which its first record names.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum Environment {
    Ci,
    Local,
    Release,
}

impl Environment {
    /// `ci` when the `CI` variable that CI services set is present, otherwise `local`.
    pub(crate) fn detect() -> Self {
        if env::var_os("CI").is_some_and(|value| !value.is_empty()) {
            Self::Ci
        } else {
            Self::Local
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Ci => "ci",
            Self::Local => "local",
            Self::Release => "release",
        }
    }
}

/// Write the trace to `path`, or to stderr for `-`, and record its first event.
///
/// Only the Tenet and ev-grep crates' records at INFO are kept: the HTTP client's own records name
/// endpoints and connections. Each span records its duration when it closes.
pub(crate) fn install(path: &Path, environment: Environment) -> Result<(), CommandError> {
    let writer = if path.as_os_str() == "-" {
        BoxMakeWriter::new(output::open_stderr)
    } else {
        BoxMakeWriter::new(Arc::new(File::create(path)?))
    };
    tracing_subscriber::registry()
        // The prefixes also select tenet_engine, tenet_contracts, ev_grep_core and ev_grep_jev.
        .with(
            Targets::new()
                .with_target("tenet", Level::INFO)
                .with_target("ev_grep", Level::INFO),
        )
        .with(
            fmt::layer()
                .json()
                .with_writer(writer)
                .with_span_events(FmtSpan::CLOSE),
        )
        .init();
    tracing::info!(
        event = LogEvent::TraceStarted.name(),
        trace_schema_version = TRACE_SCHEMA_VERSION,
        version = env!("CARGO_PKG_VERSION"),
        revision = env!("TENET_REVISION"),
        environment = environment.name(),
    );
    Ok(())
}

/// The whole check; its `exit_code` is recorded when the check ends.
///
/// A valid W3C `TRACEPARENT`, such as an evaluation harness passes, names the run's parent trace
/// and span. An invalid one is ignored, as the W3C Trace Context specification requires.
pub(crate) fn check() -> Span {
    let parent = env::var("TRACEPARENT").ok().and_then(|header| {
        let mut parts = header.split('-');
        let (version, trace, span, flags) =
            (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
        let hex = |part: &str, length: usize| {
            part.len() == length
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        let valid = version == "00"
            && parts.next().is_none()
            && hex(trace, 32)
            && hex(span, 16)
            && hex(flags, 2)
            && trace.bytes().any(|byte| byte != b'0')
            && span.bytes().any(|byte| byte != b'0');
        valid.then(|| (trace.to_owned(), span.to_owned()))
    });
    let (parent_trace_id, parent_span_id) = parent.unzip();
    tracing::info_span!(
        "tenet.check",
        parent_trace_id,
        parent_span_id,
        exit_code = field::Empty
    )
}

/// Record a request that returned no answer, by its kind and whether a rerun may succeed.
pub(crate) fn record_request_failed(failure: RequestFailure) {
    let event = if failure.kind() == FailureKind::RateLimited {
        LogEvent::RequestRateLimited
    } else {
        LogEvent::RequestFailed
    };
    tracing::info!(
        event = event.name(),
        kind = failure.kind().as_str(),
        retryable = failure.retryable(),
    );
}

/// Record the request failure that stopped the check before it concluded every contract, when
/// `error` is one.
pub(crate) fn record_stopped(error: &CommandError) {
    let CommandError::Check(
        CheckError::FileRequest { failure, .. } | CheckError::CombinedRequest { failure, .. },
    ) = error
    else {
        return;
    };
    tracing::info!(
        event = LogEvent::CheckStopped.name(),
        kind = failure.kind().as_str(),
        retryable = failure.retryable(),
    );
}
