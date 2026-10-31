mod contract;
mod display;
mod event;
mod state;
mod summary;

use alloc::collections::BTreeMap;
use std::{
    env,
    io::Write as _,
    path::{Path, PathBuf},
    time::Instant,
};

use console::Term;
use state::ContractProgress;
use tenet_contracts::Contract;
use tenet_engine::{Event, Summary};

use crate::{
    cli::ReporterKind,
    diagnostics,
    error::CommandError,
    output,
    progress::Progress,
    report_format::request_failure,
    telemetry,
};

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Fields and methods are grouped by the stage of a check that uses them."
)]
#[expect(
    clippy::partial_pub_fields,
    reason = "The private fields hold state that only this module may set."
)]
pub(crate) struct Reporter<'check> {
    kind: ReporterKind,
    contracts: &'check [Contract],
    /// The review state directory, named with the revision a check appends.
    state: &'check Path,
    term: Term,
    live: bool,
    running: bool,
    progress: Progress,
    started: Instant,
    failures: Vec<diagnostics::Failure>,
    active_contracts: BTreeMap<String, ContractProgress>,
    pub summary: Summary,
    contract_statuses: BTreeMap<&'static str, usize>,
    /// Contracts a record settled rather than the model.
    recorded: usize,
    /// Contract documents the diff deletes, listed with the summary.
    deleted: Vec<PathBuf>,
    diff: bool,
    /// The run's confidence threshold, shown beside an unresolved answer that fell short of it.
    min_confidence: f64,
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Fields and methods are grouped by the stage of a check that uses them."
)]
impl<'check> Reporter<'check> {
    pub(crate) fn new(
        kind: ReporterKind,
        contracts: &'check [Contract],
        state: &'check Path,
    ) -> Self {
        let term = output::open_terminal();
        let live = term.is_term()
            && env::var_os("CI").is_none()
            && env::var("TERM").as_deref() != Ok("dumb")
            && kind == ReporterKind::Default;
        Self {
            kind,
            contracts,
            state,
            term,
            live,
            running: false,
            progress: Progress::default(),
            started: Instant::now(),
            failures: Vec::new(),
            active_contracts: BTreeMap::new(),
            summary: Summary::default(),
            contract_statuses: BTreeMap::new(),
            recorded: 0,
            deleted: Vec::new(),
            diff: false,
            min_confidence: tenet_engine::MIN_CONFIDENCE,
        }
    }

    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The remaining variants carry no failed request."
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "One event's tracking, tracing, output channel and progress redraw stay ordered."
    )]
    pub(crate) fn emit(&mut self, event: Event) -> Result<(), CommandError> {
        self.track(&event);
        let unanswered = match &event {
            Event::Result { result } => {
                result.failure.map(|failure| {
                    (
                        &result.contract,
                        result.path.as_str(),
                        failure,
                        &result.reason,
                    )
                })
            },
            Event::ContractFinished {
                contract,
                conclusion: Some(conclusion),
                ..
            } => {
                conclusion
                    .failure
                    .map(|failure| (contract, "combined assessment", failure, &conclusion.error))
            },
            _ => None,
        };
        if let Some((_, _, failure, _)) = unanswered {
            telemetry::record_request_failed(failure);
        }
        if self.kind == ReporterKind::Jsonl {
            let failed = unanswered.map(|(contract, subject, request, message)| {
                format!(
                    "error: {contract} > {subject}: request failed ({}): {}",
                    request_failure(request),
                    message.as_deref().unwrap_or_default()
                )
            });
            let mut out = output::open_stdout().lock();
            output::write_document(
                &mut out,
                tenet_engine::EVENTS_VERSION,
                serde_json::to_value(event)?,
            )?;
            out.flush()?;
            // An agent that reads only stderr still learns which requests failed.
            if let Some(line) = failed {
                writeln!(output::open_stderr(), "{line}")?;
            }
            return Ok(());
        }
        let redraw = match event {
            Event::ContractStarted { .. }
            | Event::ContractFinished { .. }
            | Event::Skipped { .. }
            | Event::Error { .. } => true,
            Event::Begin { .. }
            | Event::CheckStarted { .. }
            | Event::Result { .. }
            | Event::StageCompleted { .. }
            | Event::StageStarted { .. }
            | Event::Summary { .. } => false,
        };
        let clears = match event {
            Event::Begin { .. }
            | Event::ContractFinished { .. }
            | Event::Skipped { .. }
            | Event::Summary { .. }
            | Event::Error { .. } => true,
            Event::CheckStarted { .. }
            | Event::ContractStarted { .. }
            | Event::Result { .. }
            | Event::StageCompleted { .. }
            | Event::StageStarted { .. } => false,
        };
        if clears {
            self.progress.clear(&self.term)?;
        }
        self.write_event(event)?;
        if redraw {
            self.refresh()?;
        }
        self.term.flush()?;
        Ok(())
    }

    pub(crate) const fn is_live(&self) -> bool {
        self.live
    }
}

impl Drop for Reporter<'_> {
    fn drop(&mut self) {
        // A write that fails on the way out has nowhere left to be reported.
        drop(self.progress.clear(&self.term));
        drop(self.term.flush());
    }
}
