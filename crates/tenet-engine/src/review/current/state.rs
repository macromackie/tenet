//! Recaptured contract rows and their path-selection rules.

use std::path::PathBuf;

use tenet_contracts::Contract;

use super::super::{Assessment, ReviewError, Revision, ScopedFile};
use crate::{Change, ContractState, Mode, RequestedScope};

/// No check compares the working tree with this base commit yet.
#[derive(Debug, thiserror::Error)]
#[error("no check compares the working tree with {base:?}; run tenet check --base {base:?}")]
pub struct Unchecked {
    pub base: String,
}

/// The current state of every contract that a review has checked.
#[derive(Debug)]
pub struct Current<'review> {
    /// One row per contract and standing path selection, in contract order.
    pub contracts: Vec<CurrentContract<'review>>,
    /// Contract documents the change deletes; their requirements left the review.
    pub deleted: Vec<PathBuf>,
    /// The latest check; its base commit identifies the review.
    pub latest: &'review Revision,
    /// A diff review whose working tree has no reviewable change from its base.
    pub unchanged: bool,
}

/// The current state of one contract in a review.
#[derive(Debug)]
pub struct CurrentContract<'review> {
    /// The assessment of exactly this input, if a check made one.
    pub assessment: Option<&'review Assessment>,
    pub changes: Vec<Change>,
    pub contract: Contract,
    pub files: Vec<ScopedFile>,
    pub input: String,
    /// The confidence threshold that routed the model's conclusion for this input.
    pub min_confidence: Option<f64>,
    pub mode: Mode,
    pub scope: RequestedScope,
    /// The latest check of this row's path selection, recaptured to compute its input.
    pub selection: &'review Revision,
    pub state: ContractState,
}

impl<'review> Current<'review> {
    /// The contract's row for these selected paths. Without paths, its only row, or the row of a
    /// check that selected everything.
    #[inline]
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    pub fn select(
        &self,
        name: &str,
        paths: &[PathBuf],
    ) -> Result<&CurrentContract<'review>, ReviewError> {
        let rows: Vec<_> = self
            .contracts
            .iter()
            .filter(|row| row.contract.name == name)
            .collect();
        if rows.is_empty() {
            return Err(ReviewError::NotInReview(name.to_owned()));
        }
        if let [only] = rows[..]
            && paths.is_empty()
        {
            return Ok(only);
        }
        rows.iter()
            .copied()
            .find(|row| row.selection.is_selecting(paths))
            .ok_or_else(|| {
                let choices: Vec<_> = rows
                    .iter()
                    .map(|row| {
                        match &*row.selection.paths {
                            [] => "(none: every path)".to_owned(),
                            listed => {
                                listed
                                    .iter()
                                    .map(|path| path.display().to_string())
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            },
                        }
                    })
                    .collect();
                ReviewError::Ambiguous {
                    name: name.to_owned(),
                    choices: choices.join("; "),
                }
            })
    }
}
