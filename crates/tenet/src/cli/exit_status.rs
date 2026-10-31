use std::process::ExitCode;

use tenet_engine::review::OverallStatus;

/// How a command ended.
///
/// Its conversion to `u8` is the one place each exit status's number is
/// written. `check` and `status` end with a review's overall status; `outdated` and `update` fail
/// when a copy needs attention.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "The declared order is the help and argument order, and ExitStatus follows its exit \
              codes."
)]
pub(crate) enum ExitStatus {
    /// Every contract is settled without a violation, or the command succeeded.
    Passed,
    /// A settled violation, or an installed copy the user has to resolve.
    Failed,
    /// Invalid input or configuration, or another execution error.
    Error,
    /// A recorded unresolved conclusion.
    Unresolved,
    /// A contract is pending.
    Pending,
    /// The command was cancelled.
    Cancelled,
}

impl From<OverallStatus> for ExitStatus {
    fn from(status: OverallStatus) -> Self {
        match status {
            OverallStatus::Passed => Self::Passed,
            OverallStatus::Unresolved => Self::Unresolved,
            OverallStatus::Failed => Self::Failed,
            OverallStatus::Pending => Self::Pending,
            OverallStatus::Error => Self::Error,
            OverallStatus::Cancelled => Self::Cancelled,
        }
    }
}

/// The number that JSON output's `exit_code` reports and the process returns.
impl From<ExitStatus> for u8 {
    #[inline]
    fn from(status: ExitStatus) -> Self {
        match status {
            ExitStatus::Passed => 0,
            ExitStatus::Failed => 1,
            ExitStatus::Error => 2,
            ExitStatus::Unresolved => 3,
            ExitStatus::Pending => 4,
            ExitStatus::Cancelled => 130,
        }
    }
}

impl From<ExitStatus> for ExitCode {
    #[inline]
    fn from(status: ExitStatus) -> Self {
        Self::from(u8::from(status))
    }
}
