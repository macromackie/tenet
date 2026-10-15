//! Raw model answers keyed by the exact request. A cache: deleting it costs only requests.

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

use anyhow::Result;
use ev_grep_core::{Assessment, Outcome, Question};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::store;

#[derive(Clone, Deserialize, Serialize)]
struct Answer {
    choice: Outcome,
    confidence: f64,
    probabilities: BTreeMap<Outcome, f64>,
    model: String,
}

#[derive(Deserialize, Serialize)]
struct Line {
    key: String,
    answers: BTreeMap<String, Answer>,
}

pub(crate) struct Answers {
    /// The provider and model that answer misses; part of every key.
    identity: String,
    reuse: bool,
    entries: RefCell<HashMap<String, BTreeMap<String, Answer>>>,
    file: Option<PathBuf>,
}

impl Answers {
    pub(super) fn new(file: Option<PathBuf>) -> Self {
        Self {
            identity: String::new(),
            reuse: false,
            entries: RefCell::default(),
            file,
        }
    }

    /// Load earlier answers for this provider and model, or only record new ones when `reuse` is off.
    pub(super) fn load(&mut self, identity: &str, reuse: bool) -> Result<()> {
        self.identity = identity.to_owned();
        self.reuse = reuse;
        if let (true, Some(file)) = (reuse, &self.file) {
            let mut entries = self.entries.borrow_mut();
            for value in store::read(file)? {
                let line: Line = serde_json::from_value(value)?;
                entries.insert(line.key, line.answers);
            }
        }
        Ok(())
    }

    /// Everything that determines an answer: Tenet's version, which pins the provider adapter and
    /// its prompt, the model, each question, and the encoded input.
    pub(crate) fn key(&self, questions: &[Question<'_>], state: &Value) -> String {
        let questions: Vec<_> = questions.iter().map(|q| [q.id, q.query]).collect();
        let request = serde_json::json!({
            "tenet": env!("CARGO_PKG_VERSION"),
            "model": self.identity,
            "questions": questions,
            "state": state,
        });
        blake3::hash(request.to_string().as_bytes())
            .to_hex()
            .to_string()
    }

    pub(crate) fn get(&self, key: &str) -> Option<BTreeMap<String, Assessment>> {
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

    pub(crate) fn put(&self, key: &str, answers: &BTreeMap<String, Assessment>) -> Result<()> {
        let answers: BTreeMap<_, _> = answers
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
            .collect();
        if let Some(file) = &self.file {
            let line = serde_json::to_value(Line {
                key: key.to_owned(),
                answers: answers.clone(),
            })?;
            store::append(file, vec![line])?;
        }
        self.entries.borrow_mut().insert(key.to_owned(), answers);
        Ok(())
    }
}
