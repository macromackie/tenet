use core::{iter, mem};
use std::path::Path;

use ev_grep_core::{ContextProfile, Evaluator, MAX_QUERY_BYTES, Question};
use serde_json::Value;

use super::super::{CheckError, assess, prompt};
use crate::{CheckResult, ClassifyRequest, Event, Stage, evidence::hash_content, review::Answers};

const QUESTION_KEY_HEX_DIGITS: usize = 48;

#[derive(Default)]
pub(super) struct Pack {
    pairs: Vec<Pair>,
}

pub(super) struct Pair {
    pub ids: [String; 2],
    pub key: String,
    pub queries: [String; 2],
    pub result: CheckResult,
    pub state: Value,
}

impl Pack {
    pub(super) fn can_push<Assessor>(
        &self,
        evaluator: &Assessor,
        maximum: usize,
        profile: ContextProfile,
        pair: &Pair,
    ) -> Result<bool, CheckError<Assessor::Error>>
    where
        Assessor: Evaluator,
    {
        let Some(first) = self.pairs.first() else {
            return Ok(true);
        };
        if self.pairs.len() >= maximum || first.state != pair.state {
            return Ok(false);
        }
        let questions = self
            .pairs
            .iter()
            .chain(iter::once(pair))
            .flat_map(|member| member.ids.iter().zip(&member.queries))
            .map(|(id, query)| Question { id, query })
            .collect::<Vec<_>>();
        let bytes = evaluator
            .measure_questions(&questions, &first.state)
            .map_err(CheckError::Measure)?;
        Ok(bytes <= profile.read_max_request_bytes())
    }

    pub(super) fn push(&mut self, pair: Pair) {
        self.pairs.push(pair);
    }

    pub(super) fn take(&mut self) -> Vec<Pair> {
        mem::take(&mut self.pairs)
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "The representation is admitted before creating a result or emitting its start."
)]
pub(super) fn prepare<Assessor, Emit, Halted>(
    context: &assess::Context<'_, Assessor, Emit>,
    path: &Path,
) -> Result<Option<Pair>, Halted>
where
    Assessor: Evaluator<Error: ClassifyRequest>,
    Emit: Fn(Event) -> Result<(), Halted>,
    Halted: From<CheckError<Assessor::Error>>,
{
    let Ok((mut state, omitted)) = context.evidence.build_file_input(context.contract, path) else {
        return Ok(None);
    };
    let queries = [Stage::Applicability, Stage::Verification].map(|stage| {
        format!(
            "{}\nAssess only the following contract for this question; other questions do not add \
             requirements.\n{}",
            prompt::query(stage, context.evidence.plan.mode),
            context.contract.body
        )
    });
    if queries.iter().any(|query| query.len() > MAX_QUERY_BYTES) {
        return Ok(None);
    }
    let Some(fields) = state.as_object_mut() else {
        return Ok(None);
    };
    let _contract = fields.remove("contract");
    let logical = [
        Question {
            id: "relevance",
            query: &queries[0],
        },
        Question {
            id: "violation",
            query: &queries[1],
        },
    ];
    let input_key =
        Answers::derive_profile_key(context.evidence.plan.context_profile, &logical, &state);
    let key = hash_content(&format!(
        "file-contract-batch-v1:{}:{}:{input_key}",
        context.contract.path.display(),
        context.contract.hash
    ));
    let question_key: String = key.chars().take(QUESTION_KEY_HEX_DIGITS).collect();
    let ids = [
        format!("{question_key}-relevance"),
        format!("{question_key}-violation"),
    ];
    let wire_questions = ids
        .iter()
        .zip(&queries)
        .map(|(id, query)| Question { id, query })
        .collect::<Vec<_>>();
    let wire_bytes = context
        .evaluator
        .measure_questions(&wire_questions, &state)
        .map_err(CheckError::Measure)?;
    if wire_bytes
        > context
            .evidence
            .plan
            .context_profile
            .read_max_request_bytes()
    {
        return Ok(None);
    }
    let mut result = assess::build_result(
        context.contract,
        path,
        context.evidence.plan.mode,
        context.evidence.plan.read_input_limit_bytes(),
    );
    result.evidence_hash = Some(key.clone());
    result.input_bytes = Some(
        serde_json::to_vec(&state)
            .map_err(CheckError::Encode)?
            .len(),
    );
    result.omitted = omitted;
    if let Ok(change) = context.evidence.change(path) {
        assess::set_change(&mut result, change);
    }
    (context.emit)(Event::CheckStarted {
        contract: result.contract.clone(),
        path: result.path.clone(),
    })?;
    Ok(Some(Pair {
        ids,
        key,
        queries,
        result,
        state,
    }))
}
