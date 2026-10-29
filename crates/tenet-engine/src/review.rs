//! Local review state: what each check assessed, keyed by exact input, and what agents recorded.
//!
//! `.tenet/events.jsonl` is the only source of truth. `answers.jsonl` caches raw model answers and
//! `blobs/` holds captured file contents; deleting either loses no conclusion.

mod answers;
mod current;
mod error;
mod log;
mod record;
mod store;

use std::{
    collections::HashMap,
    path::Path,
    time::{SystemTime, SystemTimeError, UNIX_EPOCH},
};

pub(crate) use answers::Answers;
pub use answers::Pruned;
pub use current::{Current, CurrentContract, Unchecked, resolve_current};
#[expect(
    clippy::module_name_repetitions,
    reason = "Callers name it from the public `review` module, beside the other crates' error \
              types."
)]
pub use error::ReviewError;
pub use log::{
    Answer,
    Assessment,
    CapturedFile,
    Conclusion,
    Entry,
    FileResult,
    LogError,
    Record,
    Revision,
    Role,
    ScopedFile,
    Span,
};
pub use record::{RecordRequest, Recorded, list_conclusions, record};
use store::Store;
pub use store::{DIRECTORY, MAX_BLOB_BYTES, StoreError};

use crate::{
    ContractState,
    ContractStatus,
    EvidencePacket,
    Mode,
    Pending,
    Plan,
    Selection,
    evidence,
    git,
};

/// The review state in `.tenet`: its log, records, and cached answers.
#[expect(
    clippy::module_name_repetitions,
    reason = "The crate root re-exports it as `tenet_engine::Review`, where the name repeats \
              nothing."
)]
#[derive(Debug)]
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "The sibling modules fill these fields, and the type stays private to its parent \
              module."
)]
pub struct Review {
    pub(crate) answers: Answers,
    end: u64,
    /// The log in the order it was written.
    entries: Vec<Entry>,
    /// The number of log lines read, and the byte offset after them.
    lines: usize,
    /// Each revision's position in `entries`; its assessments follow it.
    revisions: HashMap<u64, usize>,
    store: Option<Store>,
}

/// A contract's outcome in one check, before it is written to the log.
#[derive(Debug)]
pub struct Concluded {
    pub carried_from: Option<u64>,
    pub conclusion: Option<Conclusion>,
    pub contract: String,
    pub input: String,
    pub results: Vec<FileResult>,
    pub scope: crate::RequestedScope,
    pub state: ContractState,
}

/// The results and contract states a check produced.
#[derive(Debug)]
pub struct Checked {
    /// One per planned contract, in plan order.
    pub contracts: Vec<Concluded>,
    pub summary: crate::Summary,
}

impl Checked {
    /// Pending contracts, the one the model leans hardest toward violating first.
    #[must_use]
    #[inline]
    pub fn list_pending(&self) -> Vec<String> {
        let violation = |concluded: &Concluded| {
            let conclusion = concluded.conclusion.as_ref()?;
            conclusion.violation
        };
        let mut pending: Vec<_> = self
            .contracts
            .iter()
            .filter(|contract| contract.state.is_pending())
            .collect();
        pending.sort_by(|left, right| {
            let (left_violation, right_violation) = (
                violation(left).unwrap_or(-1.0_f64),
                violation(right).unwrap_or(-1.0_f64),
            );
            right_violation.total_cmp(&left_violation)
        });
        pending
            .into_iter()
            .map(|contract| contract.contract.clone())
            .collect()
    }

    /// Cancellation and execution errors take precedence over the review's states. A failed model
    /// request leaves its contract pending, so the states decide; a rerun sends it again.
    #[must_use]
    #[inline]
    pub fn summarize_overall(&self) -> OverallStatus {
        if self.summary.cancelled {
            OverallStatus::Cancelled
        } else if self.summary.errors > self.summary.request_failures {
            OverallStatus::Error
        } else {
            summarize_overall(self.contracts.iter().map(|contract| &contract.state))
        }
    }
}

/// What a check was asked to assess, kept so later commands can capture the same inputs again.
#[derive(Debug)]
pub struct CheckRequest<'plan> {
    pub context: &'plan [crate::SpanRequest],
    /// The confidence below which answers were routed to unresolved; absent without a model.
    pub min_confidence: Option<f64>,
    /// The provider and model that answered; absent for `check --no-model`.
    pub model: Option<&'plan str>,
    pub selection: &'plan Selection,
}

impl Review {
    fn append(&self, entry: Entry) -> Result<(), ReviewError> {
        let store = self.store.as_ref().ok_or(ReviewError::Ephemeral)?;
        Ok(store::append(
            &store.resolve_path(store::EVENTS),
            log::FORMAT,
            &[entry],
        )?)
    }

    /// Refuse an entry whose references to earlier entries `check` or `record` could not have
    /// written. A record in the current format must also pass every rule `record` applies; one
    /// Tenet 0.8 wrote keeps the rules of its time, so only its references are checked.
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    fn check(&self, entry: &Entry, format: u64) -> Result<(), ReviewError> {
        match entry {
            Entry::Revision(revision) => {
                if self.revisions.contains_key(&revision.rev) {
                    return Err(ReviewError::DuplicateRevision(revision.rev));
                }
            },
            Entry::Assessment(assessment) => {
                if let Some(carried) = assessment.carried_from {
                    let _: &Revision = self.find_revision(carried)?;
                }
            },
            Entry::Record(record) => {
                self.find_revision(record.rev)
                    .and_then(|revision| {
                        let assessment = (self.list_assessments_in(record.rev))
                            .find(|assessment| {
                                assessment.contract == record.contract
                                    && assessment.input == record.input
                            })
                            .ok_or_else(|| {
                                ReviewError::OtherInput {
                                    rev: record.rev,
                                    contract: record.contract.clone(),
                                }
                            })?;
                        if format == log::FORMAT {
                            record::validate(record, assessment, revision, self)?;
                        }
                        Ok(())
                    })
                    .map_err(|error| {
                        ReviewError::InvalidRecord {
                            id: record.id.clone(),
                            contract: record.contract.clone(),
                            reason: Box::new(error),
                        }
                    })?;
            },
        }
        Ok(())
    }

    /// Append one revision and its assessments, and keep the captured contents for later diffs.
    /// A check that repeats the latest revision's selection, inputs, and conclusions appends
    /// nothing. Returns the revision number, or `None` when this review keeps no state.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The store offset and revision number grow by one append each and stay far below \
                  the integer's limit."
    )]
    #[inline]
    #[expect(
        clippy::as_conversions,
        reason = "A usize fits in a u64 on every supported target."
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "The steps run in one order and share local state; splitting them would add \
                  forwarding functions."
    )]
    pub fn commit(
        &mut self,
        evidence: &EvidencePacket<'_>,
        request: &CheckRequest<'_>,
        checked: &Checked,
    ) -> Result<Option<u64>, ReviewError> {
        let Some(store) = &self.store else {
            return Ok(None);
        };
        let plan = evidence.plan;
        let mut files = Vec::new();
        let mut positions = HashMap::new();
        let mut assessments = Vec::new();
        for (contract, concluded) in plan.contracts.iter().zip(&checked.contracts) {
            drop(store.write_blob(&contract.document)?);
            let scoped = evidence.list_scoped_files(contract);
            let (selected, support) = log::add_files(&mut files, &mut positions, scoped);
            assessments.push(Assessment {
                rev: 0,
                contract: concluded.contract.clone(),
                contract_hash: contract.hash.clone(),
                input: concluded.input.clone(),
                scope: concluded.scope,
                conclusion: concluded.conclusion.clone(),
                carried_from: concluded.carried_from,
                selected,
                support,
                results: concluded.results.clone(),
            });
        }
        for file in files.iter().filter(|file| file.after.is_some()) {
            if let Some(after) = &evidence.change(&file.path)?.after {
                drop(store.write_blob(after)?);
            }
        }

        let selection = request.selection;
        let events = store.resolve_path(store::EVENTS);
        let mut number = 0;
        let _: u64 =
            store::append_after::<_, ReviewError>(&events, self.end, log::FORMAT, |appended| {
                self.extend(&events, appended, self.end + appended.len() as u64)?;
                let latest = self.list_revisions().next_back();
                let revision = Revision {
                    rev: latest.map_or(1, |previous| previous.rev + 1),
                    at: read_clock()?,
                    review: review_label(plan, selection.base.as_deref()),
                    base: selection.base.clone(),
                    paths: selection.paths.clone(),
                    contract: selection.contract.clone(),
                    context: request.context.to_vec(),
                    mode: plan.mode,
                    base_oid: plan.base.clone(),
                    head_oid: plan.head.clone(),
                    model: request.model.map(str::to_owned),
                    min_confidence: request.min_confidence,
                    tenet: env!("CARGO_PKG_VERSION").into(),
                    files,
                };
                if let Some(previous) = latest
                    && self.is_repeat(previous, &revision, &assessments)
                {
                    number = previous.rev;
                    return Ok(Vec::new());
                }
                number = revision.rev;
                let mut lines = vec![Entry::Revision(revision)];
                lines.extend(assessments.into_iter().map(|assessment| {
                    Entry::Assessment(Assessment {
                        rev: number,
                        ..assessment
                    })
                }));
                Ok(lines)
            })?;
        Ok(Some(number))
    }

    /// The latest record for this exact input settles the contract. Otherwise a favorable model
    /// conclusion does, unless a diff changed the contract itself: a diff check assumes the base
    /// complied with the contract, which says nothing about a new or edited requirement. Anything
    /// else waits for a record.
    #[inline]
    pub fn compute_state(
        &self,
        plan: &Plan,
        contract: &tenet_contracts::Contract,
        input: &str,
        model: Option<ContractStatus>,
    ) -> ContractState {
        if let Some(record) = self
            .list_records()
            .rev()
            .find(|record| record.contract == contract.name && record.input == input)
        {
            return ContractState::Settled {
                conclusion: record.conclusion,
                record: Some(record.id.clone()),
            };
        }
        let changed = plan.mode == Mode::Diff && plan.contract_changes.contains(&contract.path);
        match model {
            Some(
                ContractStatus::Verified
                | ContractStatus::Preserved
                | ContractStatus::Unaffected
                | ContractStatus::Clear,
            ) if changed => {
                ContractState::Pending {
                    reason: Pending::ContractChanged,
                }
            },
            Some(
                conclusion @ (ContractStatus::Verified
                | ContractStatus::Preserved
                | ContractStatus::Unaffected
                | ContractStatus::Clear),
            ) => {
                ContractState::Settled {
                    conclusion,
                    record: None,
                }
            },
            Some(ContractStatus::Failed) => {
                ContractState::Pending {
                    reason: Pending::ModelFailed,
                }
            },
            Some(ContractStatus::Unresolved) => {
                ContractState::Pending {
                    reason: Pending::ModelUnresolved,
                }
            },
            None => {
                ContractState::Pending {
                    reason: Pending::NotAssessed,
                }
            },
        }
    }

    /// Return every entry of the review log, oldest first.
    #[inline]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Read lines that follow those already read, refusing any that `check` or `record` would not
    /// have written, so an edited line cannot settle a contract.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The line count is bounded by the store file's size."
    )]
    fn extend(&mut self, path: &Path, text: &str, end: u64) -> Result<(), ReviewError> {
        let mut moved_files = HashMap::new();
        for line in store::read_lines(path, text, self.lines) {
            let (number, format, body) = line?;
            let entry = log::parse(format, body, self.find_last_revision(), &mut moved_files)
                .map_err(ReviewError::from)
                .and_then(|entry| self.check(&entry, format).map(|()| entry))
                .map_err(|error| {
                    ReviewError::Line {
                        path: path.to_owned(),
                        number,
                        source: Box::new(error),
                    }
                })?;
            if let Entry::Revision(revision) = &entry {
                let _: Option<usize> = self.revisions.insert(revision.rev, self.entries.len());
            }
            self.entries.push(entry);
        }
        self.lines += text.lines().count();
        self.end = end;
        Ok(())
    }

    /// The latest assessment of this contract with exactly this input, from any revision.
    #[inline]
    pub fn find_assessment(&self, contract: &str, input: &str) -> Option<&Assessment> {
        self.list_assessments()
            .rev()
            .find(|assessment| assessment.contract == contract && assessment.input == input)
    }

    /// The revision a following assessment belongs to: the latest, unless a record came after it.
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "The entries are nonempty when `last` returns one, and `revisions` holds \
                  positions of existing entries."
    )]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The remaining variants take no action here, and listing each would repeat the \
                  enum."
    )]
    fn find_last_revision(&mut self) -> Option<&mut Revision> {
        let position = match self.entries.last()? {
            Entry::Revision(_) => self.entries.len() - 1,
            Entry::Assessment(assessment) => self.revisions[&assessment.rev],
            Entry::Record(_) => return None,
        };
        match &mut self.entries[position] {
            Entry::Revision(revision) => Some(revision),
            _ => None,
        }
    }

    /// Return revision `rev`, or an error when the log does not hold it.
    #[expect(
        clippy::indexing_slicing,
        reason = "`revisions` holds positions of existing entries."
    )]
    #[inline]
    pub fn find_revision(&self, rev: u64) -> Result<&Revision, ReviewError> {
        match self
            .revisions
            .get(&rev)
            .map(|&position| &self.entries[position])
        {
            Some(Entry::Revision(revision)) => Ok(revision),
            _ => Err(ReviewError::UnknownRevision(rev)),
        }
    }

    /// Whether a new revision would only repeat the latest one: the same selection and model, and
    /// for every contract the same input and conclusion.
    fn is_repeat(
        &self,
        latest: &Revision,
        revision: &Revision,
        assessments: &[Assessment],
    ) -> bool {
        let same_selection = latest.is_same_review(revision)
            && latest.is_selecting(&revision.paths)
            && latest.contract == revision.contract
            && latest.context == revision.context
            && latest.mode == revision.mode
            && latest.model == revision.model
            && latest.min_confidence == revision.min_confidence;
        let previous = self.list_assessments_in(latest.rev).map(|assessment| {
            (
                &assessment.contract,
                &assessment.input,
                &assessment.conclusion,
            )
        });
        same_selection
            && previous.eq(assessments.iter().map(|assessment| {
                (
                    &assessment.contract,
                    &assessment.input,
                    &assessment.conclusion,
                )
            }))
    }

    fn keep(&self, text: &str) -> Result<String, StoreError> {
        match &self.store {
            Some(store) => store.write_blob(text),
            None => Ok(evidence::hash_content(text)),
        }
    }

    /// Return every recorded assessment, oldest first.
    #[inline]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The remaining variants take no action here, and listing each would repeat the \
                  enum."
    )]
    pub fn list_assessments(&self) -> impl DoubleEndedIterator<Item = &Assessment> {
        self.entries.iter().filter_map(|entry| {
            match entry {
                Entry::Assessment(assessment) => Some(assessment),
                _ => None,
            }
        })
    }

    /// The assessments one revision made, which follow it in the log.
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "Positions in `revisions` index existing entries, and the position after one is \
                  at most the entry count."
    )]
    #[inline]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The remaining variants take no action here, and listing each would repeat the \
                  enum."
    )]
    pub fn list_assessments_in(&self, rev: u64) -> impl Iterator<Item = &Assessment> {
        let start = self
            .revisions
            .get(&rev)
            .map_or(self.entries.len(), |position| position + 1);
        self.entries[start..].iter().map_while(|entry| {
            match entry {
                Entry::Assessment(assessment) => Some(assessment),
                _ => None,
            }
        })
    }

    /// Return every record in the log, oldest first.
    #[inline]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The remaining variants take no action here, and listing each would repeat the \
                  enum."
    )]
    pub fn list_records(&self) -> impl DoubleEndedIterator<Item = &Record> {
        self.entries.iter().filter_map(|entry| {
            match entry {
                Entry::Record(record) => Some(record),
                _ => None,
            }
        })
    }

    /// Return every revision in the log, oldest first.
    #[inline]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The remaining variants take no action here, and listing each would repeat the \
                  enum."
    )]
    pub fn list_revisions(&self) -> impl DoubleEndedIterator<Item = &Revision> {
        self.entries.iter().filter_map(|entry| {
            match entry {
                Entry::Revision(revision) => Some(revision),
                _ => None,
            }
        })
    }

    /// State for one run that keeps nothing, such as a snapshot check.
    #[must_use]
    #[inline]
    pub fn new_ephemeral() -> Self {
        Self {
            answers: Answers::new(None),
            end: 0,
            entries: Vec::new(),
            lines: 0,
            revisions: HashMap::new(),
            store: None,
        }
    }

    /// Open the review state in `directory`, creating it when needed.
    #[inline]
    pub fn open(directory: &Path) -> Result<Self, ReviewError> {
        Self::read_store(Store::create(directory)?)
    }

    /// Open existing review state without writing anything.
    #[inline]
    pub fn open_existing(directory: &Path) -> Result<Self, ReviewError> {
        let store = Store::open_existing(directory)?
            .ok_or_else(|| ReviewError::NoState(directory.to_owned()))?;
        Self::read_store(store)
    }

    /// Remove cached answers no check by this Tenet can reuse. The log and blobs are unchanged.
    #[inline]
    pub fn prune_answers(&self) -> Result<Pruned, StoreError> {
        self.answers.prune()
    }

    /// Open review state without writing anything; a project without any has an empty review.
    #[inline]
    pub fn read(directory: &Path) -> Result<Self, ReviewError> {
        match Store::open_existing(directory)? {
            Some(store) => Self::read_store(store),
            None => Ok(Self::new_ephemeral()),
        }
    }

    /// Read the captured file stored under `hash`, if the store holds it.
    #[inline]
    pub fn read_blob(&self, hash: &str) -> Result<Option<String>, ReviewError> {
        Ok(self
            .store
            .as_ref()
            .ok_or(ReviewError::NoBlobs)?
            .read_blob(hash)?)
    }

    fn read_store(store: Store) -> Result<Self, ReviewError> {
        let events = store.resolve_path(store::EVENTS);
        let mut review = Self {
            answers: Answers::new(Some(store.resolve_path(store::ANSWERS))),
            store: Some(store),
            ..Self::new_ephemeral()
        };
        let (text, end) = store::read(&events, 0)?;
        review.extend(&events, &text, end)?;
        Ok(review)
    }

    /// Answer from earlier identical requests to `provider` that it answered as `model`, unless
    /// `reuse` is off; new answers are always kept.
    #[inline]
    pub fn use_model(
        &mut self,
        provider: &str,
        model: &str,
        reuse: bool,
    ) -> Result<(), StoreError> {
        self.answers.read_reusable(provider, model, reuse)
    }
}

/// A check's or a review's overall status. The variants are declared from the lowest precedence to
/// the highest, so the greatest status of a review's contracts decides.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Variant order is the derived precedence order."
)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum OverallStatus {
    /// Every contract settled favorably.
    Passed,
    /// A contract settled unresolved.
    Unresolved,
    /// A contract settled failed.
    Failed,
    /// A contract needs a check or a record before the review is final.
    Pending,
    /// An execution error stopped part of the check.
    Error,
    /// The check was cancelled.
    Cancelled,
}

/// The overall status of a review's states: pending while anything is pending, then a settled
/// failure, then a settled unresolved conclusion, and passed when every contract settled
/// favorably.
///
/// Callers give cancellation and execution errors precedence.
#[inline]
#[expect(
    clippy::impl_trait_in_params,
    reason = "Callers pass closures and evaluators whose types they cannot name."
)]
pub fn summarize_overall<'plan>(
    states: impl IntoIterator<Item = &'plan ContractState>,
) -> OverallStatus {
    (states.into_iter())
        .map(|state| {
            match state {
                ContractState::Pending { .. } => OverallStatus::Pending,
                ContractState::Settled {
                    conclusion: ContractStatus::Failed,
                    ..
                } => OverallStatus::Failed,
                ContractState::Settled {
                    conclusion: ContractStatus::Unresolved,
                    ..
                } => OverallStatus::Unresolved,
                ContractState::Settled { .. } => OverallStatus::Passed,
            }
        })
        .max()
        .unwrap_or(OverallStatus::Passed)
}

/// The branch and the base commit the change is measured from, for display.
fn review_label(plan: &Plan, base: Option<&str>) -> String {
    let branch = git::read_branch(&plan.root)
        .filter(|branch| branch != "HEAD")
        .map(|branch| format!("{branch}: "))
        .unwrap_or_default();
    match (base, &plan.base) {
        (Some(prefix), Some(oid)) if oid.starts_with(prefix) => {
            format!("{branch}changes since {oid:.12}")
        },
        (Some(prefix), Some(oid)) => format!("{branch}changes since {prefix} ({oid:.12})"),
        _ => format!("{branch}full check"),
    }
}

pub(crate) fn read_clock() -> Result<u64, SystemTimeError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
