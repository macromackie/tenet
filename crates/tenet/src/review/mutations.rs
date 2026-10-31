use std::{
    collections::HashMap,
    io::{BufWriter, Write as _},
    path::Path,
};

use anyhow::Context as _;
use tenet_engine::{
    ContractStatus,
    Review,
    review::{self, Entry, Record, RecordRequest, Recorded},
};

use super::{LOG_VERSION, format_ago, format_paths_note, format_threshold_note};
use crate::{
    cli::{self, ExitStatus},
    error::CommandError,
    output,
};

#[expect(
    clippy::future_not_send,
    reason = "The command drives this future on one current-thread runtime and never moves it \
              across threads."
)]
pub(crate) async fn record(
    root: &Path,
    state: &Path,
    args: &cli::Record,
) -> Result<ExitStatus, CommandError> {
    let conclusion = ContractStatus::parse(&args.conclusion).with_context(|| {
        format!(
            "unknown conclusion {:?}; use verified, preserved, unaffected, clear, failed, or \
             unresolved",
            args.conclusion
        )
    })?;
    let Recorded { record, replaces } = review::record(
        root,
        state,
        &RecordRequest {
            contract: args.contract.clone(),
            paths: args.paths.clone(),
            conclusion,
            evidence: args.evidence.clone(),
            message: args.message.clone(),
        },
    )
    .await?;
    let mut out = output::open_stdout().lock();
    writeln!(
        out,
        "Recorded {}: {} {} for r{}",
        record.id,
        record.contract,
        record.conclusion.as_str(),
        record.rev
    )?;
    if let Some(earlier) = replaces.filter(|earlier| earlier.conclusion != record.conclusion) {
        writeln!(
            out,
            "It replaces record {}, which concluded {} for this input.",
            earlier.id,
            earlier.conclusion.as_str()
        )?;
    }
    if let Some(model) = record.model
        && model != record.conclusion
    {
        writeln!(
            out,
            "The model concluded {}; this record decides the contract while its input is \
             unchanged.",
            model.as_str()
        )?;
    }
    Ok(ExitStatus::Passed)
}

#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
#[expect(
    clippy::string_slice,
    reason = "Each slice bound is a character boundary: an ASCII hex hash prefix, a str::find \
              offset, or a span the parser returns."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
pub(crate) fn log(
    state: &Path,
    contract: Option<&str>,
    json: bool,
) -> Result<ExitStatus, CommandError> {
    let review = Review::open_existing(state)?;
    let shown = |name: &str| contract.is_none_or(|contract| contract == name);
    let mut out = BufWriter::new(output::open_stdout().lock());
    if json {
        for entry in review.entries() {
            let listed = match entry {
                Entry::Revision(revision) => {
                    review
                        .list_assessments_in(revision.rev)
                        .any(|assessment| shown(&assessment.contract))
                },
                Entry::Assessment(assessment) => shown(&assessment.contract),
                Entry::Record(record) => shown(&record.contract),
            };
            if listed {
                output::write_document(&mut out, LOG_VERSION, serde_json::to_value(entry)?)?;
            }
        }
        out.flush()?;
        return Ok(ExitStatus::Passed);
    }

    // A record names the one assessment its revision made of its contract.
    let mut records: HashMap<(u64, &str), Vec<&Record>> = HashMap::new();
    for record in review.list_records() {
        records
            .entry((record.rev, &record.contract))
            .or_default()
            .push(record);
    }
    let mut listed = false;
    for revision in review.list_revisions().rev() {
        let mut assessments = review
            .list_assessments_in(revision.rev)
            .filter(|assessment| shown(&assessment.contract))
            .peekable();
        if assessments.peek().is_none() {
            continue;
        }
        listed = true;
        writeln!(
            out,
            "r{}  {}  {}{}  {}{}",
            revision.rev,
            format_ago(revision.at),
            revision.review,
            format_paths_note(revision),
            revision.model.as_deref().unwrap_or("no model"),
            format_threshold_note(revision.min_confidence)
        )?;
        for assessment in assessments {
            let reused = assessment
                .carried_from
                .map(|rev| format!(" · reused from r{rev}"))
                .unwrap_or_default();
            writeln!(
                out,
                "  {}  model {}{reused}  input {}",
                assessment.contract,
                assessment
                    .conclusion
                    .as_ref()
                    .map_or("not assessed", |conclusion| conclusion.status.as_str()),
                &assessment.input[..12]
            )?;
            let made = records.get(&(revision.rev, assessment.contract.as_str()));
            for record in made.into_iter().flatten() {
                writeln!(
                    out,
                    "    record {}  {}  {}  {}",
                    record.id,
                    record.conclusion.as_str(),
                    format_ago(record.at),
                    record.message.replace('\n', "\n      ")
                )?;
            }
        }
    }
    if !listed {
        writeln!(out, "No checks recorded.")?;
    }
    out.flush()?;
    Ok(ExitStatus::Passed)
}
