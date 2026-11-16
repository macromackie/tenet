use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};

use super::{EvidencePacket, InputError};
use crate::{HunkRange, change};

/// A bounded group of complete diff hunks, or the exact hunk that could not fit.
#[derive(Debug, Serialize)]
pub struct HunkInput {
    input_bytes: usize,
    input_limit_bytes: usize,
    #[serde(skip)]
    patch: String,
    ranges: Vec<HunkRange>,
    #[serde(skip)]
    total_hunks: usize,
}

impl HunkInput {
    pub(crate) const fn input_bytes(&self) -> usize {
        self.input_bytes
    }

    pub(crate) const fn input_limit_bytes(&self) -> usize {
        self.input_limit_bytes
    }

    /// Whether this group can be submitted under the assessment input limit.
    #[must_use]
    #[inline]
    pub const fn is_fitting(&self) -> bool {
        self.input_bytes <= self.input_limit_bytes
    }

    pub(crate) fn ranges(&self) -> &[HunkRange] {
        &self.ranges
    }
}

impl EvidencePacket<'_> {
    #[expect(
        clippy::indexing_slicing,
        reason = "The assigned keys belong to the object built by build_hunk_state."
    )]
    pub(crate) fn build_hunk_input(
        &self,
        contract: &tenet_contracts::Contract,
        path: &Path,
        hunk: &HunkInput,
    ) -> Result<Value, InputError> {
        let mut state = self.build_hunk_state(contract, path, hunk.total_hunks)?;
        state["change"]["patch"] = json!(hunk.patch);
        state["fragment"]["ranges"] = json!(hunk.ranges);
        change::check_size(
            "hunk input",
            serde_json::to_vec(&state)?.len(),
            self.plan.read_input_limit_bytes(),
        )?;
        Ok(state)
    }

    /// Prepare advisory hunk checks from captured source after an individual diff exceeds its
    /// input limit. Every group retains complete hunks and explicit supporting context.
    #[expect(
        clippy::too_many_lines,
        reason = "Packing retains complete hunks and measures every candidate against the same \
                  captured context."
    )]
    #[expect(
        clippy::indexing_slicing,
        reason = "The assigned keys belong to the object built in this function."
    )]
    #[inline]
    pub fn build_hunk_inputs(
        &self,
        contract: &tenet_contracts::Contract,
        path: &Path,
    ) -> Result<Vec<HunkInput>, InputError> {
        let hunks = self.change(path)?.diff_hunks();
        let total_hunks = hunks.len();
        let input_limit_bytes = self.plan.read_input_limit_bytes();
        let mut state = self.build_hunk_state(contract, path, total_hunks)?;
        let mut groups = Vec::new();
        let mut ranges = Vec::new();
        let mut combined = String::new();
        for hunk in hunks {
            let previous_length = combined.len();
            combined.push_str(&hunk.patch);
            ranges.push(hunk.range);
            state["change"]["patch"] = json!(combined);
            state["fragment"]["ranges"] = json!(ranges);
            let mut input_bytes = serde_json::to_vec(&state)?.len();
            if input_bytes > input_limit_bytes && ranges.len() > 1 {
                combined.truncate(previous_length);
                let _previous = ranges.pop();
                state["change"]["patch"] = json!(combined);
                state["fragment"]["ranges"] = json!(ranges);
                let previous_bytes = serde_json::to_vec(&state)?.len();
                groups.push(HunkInput {
                    input_bytes: previous_bytes,
                    input_limit_bytes,
                    patch: combined,
                    ranges,
                    total_hunks,
                });
                ranges = vec![hunk.range];
                combined = hunk.patch;
                state["change"]["patch"] = json!(combined);
                state["fragment"]["ranges"] = json!(ranges);
                input_bytes = serde_json::to_vec(&state)?.len();
            }
            if input_bytes > input_limit_bytes {
                groups.push(HunkInput {
                    input_bytes,
                    input_limit_bytes,
                    patch: combined,
                    ranges,
                    total_hunks,
                });
                ranges = Vec::new();
                combined = String::new();
            }
        }
        if !ranges.is_empty() {
            let input_bytes = serde_json::to_vec(&state)?.len();
            groups.push(HunkInput {
                input_bytes,
                input_limit_bytes,
                patch: combined,
                ranges,
                total_hunks,
            });
        }
        Ok(groups)
    }

    fn build_hunk_state(
        &self,
        contract: &tenet_contracts::Contract,
        path: &Path,
        total_hunks: usize,
    ) -> Result<Value, InputError> {
        let change = self.change(path)?;
        let context = self
            .plan
            .context
            .iter()
            .filter(|support| support.path != change.path)
            .map(|support| self.plan.build_context_input(support))
            .collect::<Result<Vec<_>, _>>()?;
        let contents = if let Some(excerpt) = self.plan.excerpts.get(&change.path) {
            excerpt.contents()?
        } else {
            change::describe_omitted("only named hunks supplied")
        };
        Ok(json!({
            "contract": contract.body,
            "change": {
                "path": change.path,
                "previous_path": change.previous_path,
                "kind": change.kind,
                "before": change::describe_omitted("only named hunks supplied"),
                "patch": "",
            },
            "files": [{
                "path": change.path,
                "contents": contents,
            }],
            "context": context,
            "fragment": {
                "ranges": [],
                "total_hunks": total_hunks,
                "scope": "advisory evidence about these hunks; the whole file remains incomplete",
            },
        }))
    }
}
