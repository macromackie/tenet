//! Persisted answer-cache lines and their version/provider/model reuse boundary.

use alloc::collections::BTreeMap;

use ev_grep_core::Outcome;
use serde::{Deserialize, Serialize};

pub(super) const FORMAT: u64 = 2;
/// The Tenet version that asks, which pins the provider adapter and its prompt.
pub(super) const TENET: &str = env!("CARGO_PKG_VERSION");

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Answer {
    pub choice: Outcome,
    pub confidence: f64,
    pub probabilities: BTreeMap<Outcome, f64>,
    /// The model revision the provider reported for this answer.
    pub model: String,
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
/// One reusable assessment's raw answers, with its original shared request identity when present.
#[derive(Deserialize, Serialize)]
pub(super) struct Line {
    pub tenet: String,
    pub provider: String,
    pub key: String,
    pub answers: BTreeMap<String, Answer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_key: Option<String>,
}

impl Line {
    /// Whether a check by this Tenet that asks `provider` for `model` may reuse these answers:
    /// the provider must have reported that model revision for each of them.
    pub(super) fn is_reusable(&self, provider: &str, model: &str) -> bool {
        self.tenet == TENET
            && self.provider == provider
            && self.answers.values().all(|answer| answer.model == model)
    }
}
