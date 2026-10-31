use alloc::sync::Arc;
use core::mem;
use std::path::Path;

use super::{CaptureError, ChangeError, EvidencePacket, InputError};
use crate::{
    Change,
    Omitted,
    change,
    change::{SourceError, Unreadable},
};

impl EvidencePacket<'_> {
    /// The model input for one file's check; an empty file has none. When the complete input
    /// exceeds the limit, prior and then resulting contents are left out, but only when the patch
    /// remains to show the change. Lines of the file named with `--context` stand in for its
    /// resulting contents where they fit.
    #[expect(
        clippy::indexing_slicing,
        reason = "This function builds `state` with a `files` array that holds one file and a \
                  `change` object, so serde_json's index finds both."
    )]
    #[inline]
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    pub fn build_file_input(
        &self,
        contract: &tenet_contracts::Contract,
        path: &Path,
    ) -> Result<(serde_json::Value, Vec<Omitted>), InputError> {
        let change = self.change(path)?;
        if change.is_empty() {
            return Err(Unreadable::Empty.into());
        }
        let context: Vec<_> = self
            .plan
            .context
            .iter()
            .filter(|support| support.path != change.path)
            .map(|support| self.plan.build_context_input(support))
            .collect::<Result<_, _>>()?;
        let mut state = serde_json::json!({
            "contract": contract.body,
            "change": change,
            "files": [change.describe_current_input()],
            "context": context,
        });
        let mut omitted = Vec::new();
        let patched = !change.patch.is_empty();
        let excerpt = self.plan.excerpts.get(&change.path);
        for (part, present) in [
            (Omitted::Before, change.before.is_some() && patched),
            (Omitted::Contents, change.after.is_some() && patched),
        ] {
            if !present || serde_json::to_vec(&state)?.len() <= crate::MAX_FILE_BYTES {
                continue;
            }
            let too_large = change::describe_omitted("too large; judge from the patch");
            match part {
                Omitted::Before => state["change"]["before"] = too_large,
                Omitted::Contents => {
                    state["files"][0]["contents"] = too_large;
                    // The named lines stand in for the contents only where they fit.
                    if let Some(named) = excerpt {
                        let placeholder =
                            mem::replace(&mut state["files"][0]["contents"], named.contents()?);
                        if serde_json::to_vec(&state)?.len() > crate::MAX_FILE_BYTES {
                            state["files"][0]["contents"] = placeholder;
                        }
                    }
                },
            }
            omitted.push(part);
        }
        change::check_size(
            "file input with only its patch",
            serde_json::to_vec(&state)?.len(),
        )?;
        Ok((state, omitted))
    }

    pub(crate) fn change(&self, path: &Path) -> Result<&Change, ChangeError> {
        match self.changes.get(path) {
            Some(Ok(change)) => Ok(change),
            Some(Err(error)) => {
                Err(match &**error {
                    CaptureError::Source(SourceError::Unreadable(reason)) => {
                        ChangeError::Unreadable {
                            path: path.to_owned(),
                            reason: *reason,
                        }
                    },
                    CaptureError::Source(_) | CaptureError::Excerpt(_) => {
                        ChangeError::Uncaptured {
                            path: path.to_owned(),
                            error: Arc::clone(error),
                        }
                    },
                })
            },
            None => Err(ChangeError::NotCaptured(path.to_owned())),
        }
    }
}
