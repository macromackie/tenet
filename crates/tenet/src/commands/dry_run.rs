use std::{io::Write as _, path::Path};

use anyhow::Context as _;
use tenet_engine::{Mode, Plan, PreparedSnapshot, Review, Selection, Status};

use crate::{
    cli::{Check, ExitStatus, ReporterKind},
    error::CommandError,
    output,
    report_format,
};

#[expect(
    clippy::future_not_send,
    reason = "The command drives this future on one current-thread runtime and never moves it \
              across threads."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
#[expect(
    clippy::excessive_nesting,
    reason = "The match and callback levels keep one command decision together."
)]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Counts are bounded by the repository and request budget; subtraction follows its \
              bound check."
)]
#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
#[expect(
    clippy::cognitive_complexity,
    reason = "The report keeps file selection, combined inputs and their request accounting in \
              one order."
)]
pub(super) async fn report_dry_run(
    plan: &Plan,
    selection: &Selection,
    snapshot: Option<&PreparedSnapshot>,
    review_state: &Path,
    check: &Check,
    kind: ReporterKind,
) -> Result<ExitStatus, CommandError> {
    let run = &check.run;
    let budget = run.request_budget().context("convert request budget")?;
    let evidence = plan.capture().await?;
    // Answers a check would reuse; reading the review writes nothing.
    let mut review = if snapshot.is_some() {
        Review::new_ephemeral()
    } else {
        Review::read(review_state)?
    };
    review.use_model(
        &run.provider.to_string(),
        &run.provider.select_model(run.model.as_deref())?,
        !check.fresh,
    )?;
    let prediction = tenet_engine::estimate(&evidence, &review, run.jobs.into()).await?;
    let mut errors = 0;
    // Contracts whose combined input stays over the limit after reduction.
    let mut over_limit = 0_i32;
    for contract in &plan.contracts {
        let scope = (kind == ReporterKind::Jsonl).then(|| plan.find_requested_scope(contract));
        for path in plan
            .files
            .iter()
            .filter(|path| plan.is_governing(contract, path))
        {
            let previous_path = plan.previous_paths.get(path);
            let input = evidence.build_file_input(contract, path);
            let (input_bytes, omitted, note) = match input {
                Ok((state, omitted)) => (Some(serde_json::to_vec(&state)?.len()), omitted, None),
                Err(error) => {
                    let (status, reason) = tenet_engine::classify_input_status(&error);
                    errors += usize::from(status == Status::Error);
                    (None, Vec::new(), Some((status, reason)))
                },
            };
            if kind == ReporterKind::Jsonl {
                let (status, reason) = note.unzip();
                output::write_document(
                    &mut output::open_stdout(),
                    tenet_engine::EVENTS_VERSION,
                    serde_json::json!({
                        "type": "selected",
                        "contract": contract.name,
                        "scope": scope,
                        "path": path,
                        "previous_path": previous_path,
                        "contract_hash": contract.hash,
                        "input_bytes": input_bytes,
                        "input_limit_bytes": tenet_engine::MAX_FILE_BYTES,
                        "omitted": omitted,
                        "status": status,
                        "reason": reason,
                    }),
                )?;
            } else {
                let note = match (&note, omitted.as_slice()) {
                    (Some((status, reason)), _) => {
                        format!(
                            "  {}: {reason}",
                            report_format::label_status(*status).to_lowercase()
                        )
                    },
                    (None, []) => String::new(),
                    (None, omitted) => format!("  {}", report_format::format_omitted(omitted)),
                };
                let renamed = previous_path
                    .map(|old| format!(" (renamed from {})", old.display()))
                    .unwrap_or_default();
                writeln!(
                    output::open_stdout(),
                    "{}  {}{renamed}{note}",
                    contract.name,
                    path.display()
                )?;
            }
        }
        let context = evidence.summarize_combined(contract);
        if let Ok(context) = &context
            && !plan.is_untouched(contract)
            && !context.is_fitting()
        {
            over_limit += 1_i32;
        }
        if kind == ReporterKind::Jsonl {
            let (context, error) = match context {
                Ok(context) => (Some(context), None),
                Err(error) => (None, Some(format!("{error:#}"))),
            };
            output::write_document(
                &mut output::open_stdout(),
                tenet_engine::EVENTS_VERSION,
                serde_json::json!({
                    "type": "combined",
                    "contract": contract.name,
                    "context": context,
                    "error": error,
                }),
            )?;
        } else {
            writeln!(
                output::open_stdout(),
                "{}  combined {}",
                contract.name,
                match context {
                    Ok(context) => report_format::format_context(&context),
                    Err(error) => format!("unavailable: {error:#}"),
                }
            )?;
        }
    }
    // Answers from earlier checks spend none of the budget.
    let unsent = prediction.file_requests + prediction.combined_requests;
    let coverage = plan
        .coverage
        .as_ref()
        .and_then(report_format::format_coverage);
    let status = if errors > 0 {
        ExitStatus::Error
    } else {
        ExitStatus::Passed
    };
    if kind == ReporterKind::Jsonl {
        output::write_document(
            &mut output::open_stdout(),
            tenet_engine::EVENTS_VERSION,
            serde_json::json!({
                "type": "summary",
                "dry_run": true,
                "files": plan.files.len(),
                "mode": plan.mode,
                "assume_base_valid": plan.mode == Mode::Diff,
                "deleted": plan.deleted,
                "requests": unsent + prediction.cached,
                "cached": prediction.cached,
                "max_requests": budget,
                "combined_over_limit": over_limit,
                "coverage": plan.coverage,
                "errors": errors,
                "exit_code": u8::from(status),
            }),
        )?;
    } else if plan.contracts.is_empty() {
        // A full check with nothing to assess has already warned about its selection.
        if let Some(base) = &selection.base {
            match (coverage, plan.candidate_files.is_empty()) {
                (Some(coverage), _) => {
                    writeln!(output::open_stdout(), "{coverage}; nothing to check.")?;
                },
                (None, true) => {
                    writeln!(
                        output::open_stdout(),
                        "No changes since {base}; nothing to check."
                    )?;
                },
                (None, false) => {
                    writeln!(
                        output::open_stdout(),
                        "No contract governs the changes since {base}; nothing to check."
                    )?;
                },
            }
        }
    } else {
        writeln!(
            output::open_stdout(),
            "Requests  {unsent} to send ({} file, {} combined), {} answered from earlier checks; \
             budget {budget}\nCombined  {over_limit} of {} contracts over the input limit after \
             reduction",
            prediction.file_requests,
            prediction.combined_requests,
            prediction.cached,
            plan.contracts.len()
        )?;
        if let Some(coverage) = coverage {
            writeln!(output::open_stdout(), "{coverage}")?;
        }
    }
    if unsent > budget {
        writeln!(
            output::open_stderr(),
            "warning: {unsent} predicted requests exceed --max-requests {budget}; {} checks would \
             stay incomplete",
            unsent - budget
        )?;
    }
    Ok(status)
}
