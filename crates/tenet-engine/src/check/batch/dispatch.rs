use alloc::collections::BTreeMap;

use ev_grep_core::{Evaluator, Question};

use super::{FileAssessment, pack::Pack};
use crate::{
    ClassifyRequest,
    Event,
    FailureKind,
    RequestFailure,
    Status,
    check::{CheckError, Pool, assess},
    review::Answers,
};

#[expect(
    clippy::arithmetic_side_effects,
    reason = "The request budget bounds submitted and refused requests."
)]
#[expect(
    clippy::future_not_send,
    reason = "The checker drives this future on one task and never moves it across threads."
)]
#[expect(
    clippy::too_many_lines,
    reason = "One physical request owns admission, usage, validation, caching and failure routing."
)]
pub(super) async fn flush<Assessor, Emit, Halted>(
    pool: &Pool<'_, Assessor, Emit>,
    pending: &mut Pack,
    assessed: &mut FileAssessment,
) -> Result<(), Halted>
where
    Assessor: Evaluator<Error: ClassifyRequest>,
    Emit: Fn(Event) -> Result<(), Halted>,
    Halted: From<CheckError<Assessor::Error>>,
{
    let mut pairs = pending.take();
    let Some(first) = pairs.first() else {
        return Ok(());
    };
    let questions = pairs
        .iter()
        .flat_map(|pair| pair.ids.iter().zip(&pair.queries))
        .map(|(id, query)| Question { id, query })
        .collect::<Vec<_>>();
    let request_key =
        Answers::derive_profile_key(pool.evidence.plan.context_profile, &questions, &first.state);
    if pool.requests.get() >= pool.options.max_requests {
        assessed.work.over_budget += 1;
        for mut pair in pairs {
            pair.result.status = Status::Incomplete;
            pair.result.reason = Some("request budget exhausted".into());
            assessed.results.push(pair.result);
        }
        return Ok(());
    }
    let input_bytes = pool
        .evaluator
        .measure_questions(&questions, &first.state)
        .map_err(CheckError::Measure)?;
    pool.requests.set(pool.requests.get() + 1);
    assessed.work.requests += 1;
    (pool.emit)(Event::FileBatchStarted {
        request_key: request_key.clone(),
        path: first.result.path.clone(),
        contracts: pairs
            .iter()
            .map(|pair| pair.result.contract.clone())
            .collect(),
        questions: questions.len(),
        input_bytes,
    })?;
    let answered = pool
        .evaluator
        .assess_questions(&questions, &first.state)
        .await;
    let mut answers = match answered {
        Ok(batch) => {
            assessed.work.record_request(Some(&batch.request));
            let valid = batch.answers.len() == questions.len()
                && questions
                    .iter()
                    .all(|question| batch.answers.contains_key(question.id));
            let failure =
                (!valid).then(|| RequestFailure::new(FailureKind::InvalidResponse, false));
            (pool.emit)(Event::FileBatchCompleted {
                request_key: request_key.clone(),
                request: Some(batch.request),
                failure,
            })?;
            if let Some(invalid) = failure {
                assessed.work.record_failure(Some(invalid));
                for mut pair in pairs {
                    pair.result.shared_request = Some(request_key.clone());
                    pair.result.failure = Some(invalid);
                    pair.result.reason =
                        Some("shared request returned an unexpected answer set".into());
                    assessed.results.push(pair.result);
                }
                return Ok(());
            }
            batch.answers
        },
        Err(error) => {
            let failure = error.classify_request();
            (pool.emit)(Event::FileBatchCompleted {
                request_key: request_key.clone(),
                request: None,
                failure: Some(failure),
            })?;
            assessed.work.record_failure(Some(failure));
            if let Some(reason) = failure.explain_stop() {
                return Err(CheckError::FileRequest {
                    failure,
                    reason,
                    contract: first.result.contract.clone(),
                    path: first.result.path.clone(),
                    source: Box::new(error),
                }
                .into());
            }
            for mut pair in pairs {
                pair.result.shared_request = Some(request_key.clone());
                pair.result.failure = Some(failure);
                pair.result.reason = Some(error.to_string());
                assessed.results.push(pair.result);
            }
            return Ok(());
        },
    };
    for pair in &mut pairs {
        let applicability = answers
            .remove(&pair.ids[0])
            .ok_or(CheckError::InvalidBatchAnswers)?;
        let verification = answers
            .remove(&pair.ids[1])
            .ok_or(CheckError::InvalidBatchAnswers)?;
        let pair_answers = BTreeMap::from([
            ("relevance".into(), applicability.clone()),
            ("violation".into(), verification.clone()),
        ]);
        pool.review
            .answers
            .put_shared(&pair.key, &pair_answers, &request_key)
            .map_err(CheckError::from)?;
        pair.result.shared_request = Some(request_key.clone());
        assess::apply_answers(
            pool.options.min_confidence,
            pool.emit,
            applicability,
            verification,
            &mut pair.result,
        )?;
    }
    assessed
        .results
        .extend(pairs.into_iter().map(|pair| pair.result));
    Ok(())
}
