use std::path::Path;

use serde_json::Value;

use super::{CaptureError, ChangeError, EvidencePacket, InputError};
use crate::{Causes, ContextError};

fn build_context_change_error(failure: ChangeError) -> ContextError {
    match failure {
        ChangeError::Uncaptured { error, .. } => {
            match &*error {
                CaptureError::Excerpt(excerpt) => ContextError::Excerpt(excerpt.clone()),
                CaptureError::Limit(limit) => ContextError::Limit(*limit),
                CaptureError::Source(_) => ContextError::Unavailable(Causes(&*error).to_string()),
            }
        },
        ChangeError::Unreadable { reason, .. } => ContextError::Source(reason.into()),
        ChangeError::NotCaptured(missing) => ContextError::Missing(missing),
    }
}

fn build_context_input_error(failure: InputError) -> ContextError {
    match failure {
        InputError::Change(change) => build_context_change_error(change),
        InputError::Encode(encoding) => ContextError::Encode(encoding),
        InputError::Excerpt(excerpt) => ContextError::Excerpt(excerpt),
        InputError::Limit(limit) => ContextError::Limit(limit),
        InputError::NotUtf8Path => ContextError::Source(crate::SourceError::NotUtf8Path),
        InputError::Unreadable(reason) => ContextError::Source(reason.into()),
    }
}

impl EvidencePacket<'_> {
    /// Validate explicit context after source capture. Size exclusions remain incomplete evidence;
    /// malformed ranges, missing files and source errors still diagnose invalid supplied context.
    #[inline]
    pub fn admit_context(&self) -> Result<(), ContextError> {
        let mut inputs = Vec::new();
        for path in &self.plan.context {
            let change = self.change(path).map_err(build_context_change_error)?;
            if change.before.is_none() && change.after.is_none() {
                return Err(ContextError::Missing(path.clone()));
            }
            inputs.push(
                self.build_context_input(path)
                    .map_err(build_context_input_error)?,
            );
        }
        let bytes = super::capture::measure_context(&inputs)?;
        let limit_bytes = self.plan.read_input_limit_bytes();
        if bytes > limit_bytes {
            return Err(ContextError::TooLarge { bytes, limit_bytes });
        }
        Ok(())
    }

    pub(crate) fn build_context_input(&self, path: &Path) -> Result<Value, InputError> {
        let change = self.change(path)?;
        match self.excerpts.get(path) {
            Some(excerpt) => {
                Ok(serde_json::json!({ "path": change.path, "contents": excerpt.contents()? }))
            },
            None => Ok(change.describe_current_input()),
        }
    }
}
