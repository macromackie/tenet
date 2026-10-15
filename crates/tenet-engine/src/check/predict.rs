use std::cell::Cell;

use anyhow::{Result, bail};
use ev_grep_core::{Assessment, Candidate, DecisionBatch, Evaluator, Question};

use crate::{Event, EvidencePacket, FailureKind, MIN_CONFIDENCE, Review, Stage};

/// The model requests a check would send, and those the review's answers would answer instead.
#[derive(Clone, Copy, Debug, Default)]
pub struct Prediction {
    pub file_requests: usize,
    pub combined_requests: usize,
    pub cached: usize,
}

/// Follow `run` without sending anything. An evaluator that answers no request stands in for the
/// provider, so cached answers, inputs that send nothing, and unaffected contracts count exactly
/// as a check counts them. The budget is unlimited; the caller compares the total with its own.
pub async fn predict(
    evidence: &EvidencePacket<'_>,
    review: &Review,
    jobs: usize,
) -> Result<Prediction> {
    let options = super::Options {
        jobs,
        max_requests: usize::MAX,
        min_confidence: MIN_CONFIDENCE,
        classify: |_| FailureKind::Unknown,
    };
    let (file_requests, combined_requests) = (Cell::new(0), Cell::new(0));
    let checked = super::run(evidence, options, &Unsent, review, &|event| {
        match event {
            Event::StageStarted {
                stage: Stage::Completeness,
                ..
            } => combined_requests.set(combined_requests.get() + 1),
            Event::StageStarted {
                stage: Stage::Applicability,
                ..
            } => file_requests.set(file_requests.get() + 1),
            _ => {}
        }
        Ok(())
    })
    .await?;
    Ok(Prediction {
        file_requests: file_requests.get(),
        combined_requests: combined_requests.get(),
        cached: checked.summary.cached,
    })
}

struct Unsent;

impl Evaluator for Unsent {
    async fn assess(&self, _: &str, _: &Candidate) -> Result<Assessment> {
        bail!("a dry run sends no request")
    }

    async fn assess_questions(
        &self,
        _: &[Question<'_>],
        _: &serde_json::Value,
    ) -> Result<DecisionBatch> {
        bail!("a dry run sends no request")
    }
}
