//! Raw model answers keyed by the exact request. A cache: deleting it costs only requests.

use alloc::collections::{BTreeMap, BTreeSet};
use core::cell::RefCell;
use std::{collections::HashMap, path::PathBuf};

use ev_grep_core::{Assessment, Outcome, Question};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::store::{self, StoreError};

const FORMAT: u64 = 2;
/// The Tenet version that asks, which pins the provider adapter and its prompt.
const TENET: &str = env!("CARGO_PKG_VERSION");

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Answer {
    choice: Outcome,
    confidence: f64,
    probabilities: BTreeMap<Outcome, f64>,
    /// The model revision the provider reported for this answer.
    model: String,
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
/// One request's answers, with the identity a later check must share to reuse them.
#[derive(Deserialize, Serialize)]
struct Line {
    tenet: String,
    provider: String,
    key: String,
    answers: BTreeMap<String, Answer>,
}

impl Line {
    /// Whether a check by this Tenet that asks `provider` for `model` may reuse these answers:
    /// the provider must have reported that model revision for each of them.
    fn is_reusable(&self, provider: &str, model: &str) -> bool {
        self.tenet == TENET
            && self.provider == provider
            && self.answers.values().all(|answer| answer.model == model)
    }
}

/// How many cached requests a prune kept and removed.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pruned {
    pub kept: usize,
    pub removed: usize,
}

#[derive(Debug)]
pub(crate) struct Answers {
    entries: RefCell<HashMap<String, BTreeMap<String, Answer>>>,
    file: Option<PathBuf>,
    model: String,
    /// The provider and the model revision it must report for a cached answer to be reused.
    provider: String,
    reuse: bool,
}

impl Answers {
    /// The request: each question and the encoded input. Each cached line keeps the Tenet
    /// version, the provider, and the model revision it reported beside the key.
    pub(crate) fn derive_key(questions: &[Question<'_>], state: &Value) -> String {
        let pairs: Vec<_> = questions
            .iter()
            .map(|question| [question.id, question.query])
            .collect();
        let request = serde_json::json!({
            "questions": pairs,
            "state": state,
        });
        blake3::hash(request.to_string().as_bytes())
            .to_hex()
            .to_string()
    }

    pub(super) fn new(file: Option<PathBuf>) -> Self {
        Self {
            entries: RefCell::default(),
            file,
            model: String::new(),
            provider: String::new(),
            reuse: false,
        }
    }

    /// Remove the answers no check by this Tenet can reuse: those an earlier format or another
    /// Tenet version wrote, and those a later answer to the same request replaced. A review that
    /// keeps no state has none to remove.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "`kept` is a subset of the counted answers, so the count is at least its length."
    )]
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    pub(super) fn prune(&self) -> Result<Pruned, StoreError> {
        let Some(file) = &self.file else {
            return Ok(Pruned::default());
        };
        let mut pruned = Pruned::default();
        store::replace_lines(file, |text| {
            let mut reusable = Vec::new();
            let mut latest_lines = HashMap::new();
            let mut total = 0;
            for line in store::read_lines(file, text, 0) {
                let (number, format, stored) = line?;
                total += 1;
                if format != FORMAT {
                    continue;
                }
                let parsed: Line = serde_json::from_str(stored).map_err(|source| {
                    StoreError::InvalidLine {
                        path: file.clone(),
                        number,
                        source,
                    }
                })?;
                if parsed.tenet != TENET {
                    continue;
                }
                let models: Vec<_> = parsed
                    .answers
                    .into_values()
                    .map(|answer| answer.model)
                    .collect();
                let _: Option<usize> =
                    latest_lines.insert((parsed.provider, models, parsed.key), reusable.len());
                reusable.push(stored);
            }
            let kept_lines: BTreeSet<usize> = latest_lines.into_values().collect();
            pruned = Pruned {
                kept: kept_lines.len(),
                removed: total - kept_lines.len(),
            };
            Ok(reusable
                .iter()
                .enumerate()
                .filter(|(index, _)| kept_lines.contains(index))
                .fold(String::new(), |mut lines, (_, stored)| {
                    lines.push_str(stored);
                    lines.push('\n');
                    lines
                }))
        })?;
        Ok(pruned)
    }

    pub(crate) fn put(
        &self,
        key: &str,
        answers: &BTreeMap<String, Assessment>,
    ) -> Result<(), StoreError> {
        let line = Line {
            tenet: TENET.into(),
            provider: self.provider.clone(),
            key: key.to_owned(),
            answers: answers
                .iter()
                .map(|(id, assessment)| {
                    (
                        id.clone(),
                        Answer {
                            choice: assessment.choice,
                            confidence: assessment.confidence,
                            probabilities: assessment.probabilities.clone(),
                            model: assessment.model.clone(),
                        },
                    )
                })
                .collect(),
        };
        if let Some(file) = &self.file {
            store::append(file, FORMAT, &[&line])?;
        }
        if line.is_reusable(&self.provider, &self.model) {
            drop(self.entries.borrow_mut().insert(line.key, line.answers));
        }
        Ok(())
    }

    pub(crate) fn read(&self, key: &str) -> Option<BTreeMap<String, Assessment>> {
        if !self.reuse {
            return None;
        }
        let entries = self.entries.borrow();
        let answers = entries.get(key)?;
        Some(
            answers
                .iter()
                .map(|(id, answer)| {
                    (
                        id.clone(),
                        Assessment {
                            choice: answer.choice,
                            confidence: answer.confidence,
                            probabilities: answer.probabilities.clone(),
                            model: answer.model.clone(),
                            request: None,
                        },
                    )
                })
                .collect(),
        )
    }

    /// Load earlier answers this provider gave as this model revision, or only record new ones
    /// when `reuse` is off. Lines in an earlier format hold an earlier Tenet's answers, which no
    /// check by this one reuses.
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    pub(super) fn read_reusable(
        &mut self,
        provider: &str,
        model: &str,
        reuse: bool,
    ) -> Result<(), StoreError> {
        provider.clone_into(&mut self.provider);
        model.clone_into(&mut self.model);
        self.reuse = reuse;
        if let (true, Some(file)) = (reuse, &self.file) {
            let mut entries = self.entries.borrow_mut();
            let (text, _) = store::read(file, 0)?;
            for line in store::read_lines(file, &text, 0) {
                let (number, format, body) = line?;
                if format < FORMAT {
                    continue;
                }
                if format != FORMAT {
                    return Err(StoreError::UnsupportedFormat {
                        path: file.clone(),
                        number,
                        format,
                    });
                }
                let parsed: Line = serde_json::from_str(body).map_err(|source| {
                    StoreError::InvalidLine {
                        path: file.clone(),
                        number,
                        source,
                    }
                })?;
                if parsed.is_reusable(provider, model) {
                    drop(entries.insert(parsed.key, parsed.answers));
                }
            }
        }
        Ok(())
    }
}
