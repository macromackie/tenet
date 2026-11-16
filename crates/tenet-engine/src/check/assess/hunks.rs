use ev_grep_core::Evaluator;

use super::{Context, FileError, Halt, judge_input, set_file_error};
use crate::{
    Answer,
    Change,
    CheckError,
    CheckResult,
    ClassifyRequest,
    Event,
    HunkInput,
    HunkResult,
    Status,
    Summary,
};

#[expect(
    clippy::future_not_send,
    reason = "The checker drives this future on one task."
)]
#[expect(
    clippy::too_many_lines,
    reason = "Group execution keeps request accounting, streamed receipts and durable evidence in \
              one ordered lifecycle."
)]
// jscpd:ignore-start -- duplication -- Rust bounds retain evaluator and cancellation errors.
pub(super) async fn assess<Assessor, Emit, Halted>(
    context: &Context<'_, Assessor, Emit>,
    change: &Change,
    result: &mut CheckResult,
) -> Result<(), Halt<Halted, Assessor::Error>>
where
    Assessor: Evaluator<Error: ClassifyRequest>,
    Emit: Fn(Event) -> Result<(), Halted>,
    Halted: From<CheckError<Assessor::Error>>,
    // jscpd:ignore-end
{
    let inputs = context
        .evidence
        .build_hunk_inputs(context.contract, &change.path)
        .map_err(FileError::Input)?;
    let mut summary = Summary::default();
    let template = result.clone();
    for input in inputs {
        let mut current = template.clone();
        if input.is_fitting() {
            let state = context
                .evidence
                .build_hunk_input(context.contract, &change.path, &input)
                .map_err(FileError::Input)?;
            match judge_input(context, &state, &mut current).await {
                Ok(()) => {},
                Err(Halt::Check(error)) => return Err(Halt::Check(error)),
                Err(Halt::File(error)) => set_file_error(error, &mut current),
            }
        } else {
            current.status = Status::Incomplete;
            current.reason = Some(format!(
                "complete hunk input requires {} bytes; limit is {} bytes",
                input.input_bytes(),
                input.input_limit_bytes(),
            ));
        }
        summary.add(&current);
        let cached = current.cached;
        let request = current.request.take();
        let hunk = describe(&input, current);
        (context.emit)(Event::HunkResult {
            cached,
            contract: context.contract.name.clone(),
            path: result.path.clone(),
            request,
            result: Box::new(hunk.clone()),
        })
        .map_err(Halt::Check)?;
        result.hunks.push(hunk);
    }
    result.status = if summary.errors > 0 {
        Status::Error
    } else {
        Status::Incomplete
    };
    result.reason = Some(
        "oversized diff reviewed by complete hunks. Whole-file assessment remains incomplete"
            .into(),
    );
    result.requests = summary.requests;
    result.hunk_summary = Some(summary);
    Ok(())
}

fn describe(input: &HunkInput, result: CheckResult) -> HunkResult {
    HunkResult {
        applicability: result.applicability.as_ref().map(|answer| {
            Answer {
                choice: answer.choice,
                confidence: answer.confidence,
            }
        }),
        evidence_hash: result.evidence_hash,
        failure: result.failure,
        input_bytes: input.input_bytes(),
        input_limit_bytes: input.input_limit_bytes(),
        ranges: input.ranges().to_vec(),
        reason: result.reason,
        status: result.status,
        verification: result.verification.as_ref().map(|answer| {
            Answer {
                choice: answer.choice,
                confidence: answer.confidence,
            }
        }),
        violation: result.verification.as_ref().map(crate::estimate_violation),
    }
}
