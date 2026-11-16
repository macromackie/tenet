use core::error::Error;

use crate::{CheckResult, Status, evidence::InputError, report::Causes, review::StoreError};

/// Why one file's check produced no answer; its result reports it.
#[derive(Debug, thiserror::Error)]
pub(super) enum FileError<RequestError> {
    #[error(transparent)]
    Cache(#[from] StoreError),
    #[error(transparent)]
    Encode(#[from] serde_json::Error),
    #[error(transparent)]
    Input(#[from] InputError),
    #[error("missing {0:?} answer")]
    MissingAnswer(&'static str),
    #[error(transparent)]
    Request(RequestError),
}

/// Why judging a file ended early: a failure of the file's own check, or one that ends the check.
pub(super) enum Halt<Halted, RequestError> {
    Check(Halted),
    File(FileError<RequestError>),
}

impl<Halted, RequestError> From<FileError<RequestError>> for Halt<Halted, RequestError> {
    fn from(error: FileError<RequestError>) -> Self {
        Self::File(error)
    }
}

pub(super) fn set_file_error<RequestError>(
    failure: FileError<RequestError>,
    result: &mut CheckResult,
) where
    RequestError: Error + 'static,
{
    match failure {
        FileError::Input(error) => {
            let (status, reason) = crate::classify_input_status(&error);
            result.status = status;
            result.reason = Some(reason);
        },
        error @ (FileError::Cache(_)
        | FileError::Encode(_)
        | FileError::MissingAnswer(_)
        | FileError::Request(_)) => {
            result.status = Status::Error;
            result.reason = Some(Causes(&error).to_string());
        },
    }
}
