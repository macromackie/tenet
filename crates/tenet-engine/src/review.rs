//! Local review state: what each check assessed, keyed by exact input, and what agents recorded.
//!
//! `.tenet/events.jsonl` is the only source of truth. `answers.jsonl` caches raw model answers and
//! `blobs/` holds captured file contents; deleting either loses no conclusion.

mod answers;
mod current;
mod log;
mod record;
mod store;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail, ensure};

pub(crate) use answers::Answers;
pub use current::{Current, CurrentContract, Unchecked, current};
pub use log::{
    Answer, Assessment, CapturedFile, Conclusion, Entry, FileResult, Record, Revision, Role,
    ScopedFile, Span,
};
pub use record::{RecordRequest, Recorded, SpanRequest, conclusions, record};
pub use store::MAX_BLOB_BYTES;
use store::Store;

use crate::{ContractState, ContractStatus, EvidencePacket, Mode, Pending, Plan, Selection, git};

pub struct Review {
    store: Option<Store>,
    /// The log in the order it was written.
    entries: Vec<Entry>,
    /// Each revision's position in `entries`; its assessments follow it.
    revisions: HashMap<u64, usize>,
    /// The number of log lines read, and the byte offset after them.
    lines: usize,
    end: u64,
    pub(crate) answers: Answers,
}

/// A contract's outcome in one check, before it is written to the log.
pub struct Concluded {
    pub contract: String,
    pub input: String,
    pub scope: crate::RequestedScope,
    pub conclusion: Option<Conclusion>,
    pub carried_from: Option<u64>,
    pub results: Vec<FileResult>,
    pub state: ContractState,
}

pub struct Checked {
    pub summary: crate::Summary,
    /// One per planned contract, in plan order.
    pub contracts: Vec<Concluded>,
}

impl Checked {
    /// Cancellation (130) and execution errors (2) take precedence over the review's states.
    pub fn exit_code(&self) -> u8 {
        if self.summary.cancelled {
            130
        } else if self.summary.errors > 0 {
            2
        } else {
            exit_code(self.contracts.iter().map(|c| &c.state))
        }
    }

    /// Pending contracts, the one the model leans hardest toward violating first.
    pub fn pending(&self) -> Vec<String> {
        let violation = |c: &Concluded| c.conclusion.as_ref().and_then(|c| c.violation);
        let mut pending: Vec<_> = self
            .contracts
            .iter()
            .filter(|c| c.state.is_pending())
            .collect();
        pending.sort_by(|a, b| {
            let (a, b) = (violation(a).unwrap_or(-1.0), violation(b).unwrap_or(-1.0));
            b.total_cmp(&a)
        });
        pending.into_iter().map(|c| c.contract.clone()).collect()
    }
}

/// What a check was asked to assess, kept so later commands can capture the same inputs again.
pub struct CheckRequest<'a> {
    pub selection: &'a Selection,
    pub context: &'a [PathBuf],
    /// The provider and model that answered; absent for `check --no-model`.
    pub model: Option<&'a str>,
    /// The confidence below which answers were routed to unresolved; absent without a model.
    pub min_confidence: Option<f64>,
}

impl Review {
    /// Open the project's review state, creating `.tenet/` when needed.
    pub fn open(root: &Path) -> Result<Self> {
        Self::load(Store::create(root)?)
    }

    /// Open existing review state without writing anything.
    pub fn existing(root: &Path) -> Result<Self> {
        let store = Store::existing(root)?.with_context(|| {
            format!(
                "no review state in {}; run tenet check first",
                root.join(store::DIRECTORY).display()
            )
        })?;
        Self::load(store)
    }

    /// State for one run that keeps nothing, such as a snapshot check.
    pub fn ephemeral() -> Self {
        Self {
            store: None,
            entries: Vec::new(),
            revisions: HashMap::new(),
            lines: 0,
            end: 0,
            answers: Answers::new(None),
        }
    }

    fn load(store: Store) -> Result<Self> {
        let events = store.path(store::EVENTS);
        let mut review = Self {
            answers: Answers::new(Some(store.path(store::ANSWERS))),
            store: Some(store),
            ..Self::ephemeral()
        };
        let (text, end) = store::read(&events, 0)?;
        review.extend(&events, &text, end)?;
        Ok(review)
    }

    /// Read lines that follow those already read, refusing any that `check` or `record` would not
    /// have written, so an edited line cannot settle a contract.
    fn extend(&mut self, path: &Path, text: &str, end: u64) -> Result<()> {
        let mut moved = HashMap::new();
        for line in store::lines(path, text, self.lines) {
            let (number, format, line) = line?;
            let entry = log::parse(format, line, self.last_revision(), &mut moved)
                .and_then(|entry| self.check(&entry, format).map(|()| entry))
                .with_context(|| format!("{}:{number}", path.display()))?;
            if let Entry::Revision(revision) = &entry {
                self.revisions.insert(revision.rev, self.entries.len());
            }
            self.entries.push(entry);
        }
        self.lines += text.lines().count();
        self.end = end;
        Ok(())
    }

    /// The revision a following assessment belongs to: the latest, unless a record came after it.
    fn last_revision(&mut self) -> Option<&mut Revision> {
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

    /// Refuse an entry whose references to earlier entries `check` or `record` could not have
    /// written. A record in the current format must also pass every rule `record` applies; one
    /// Tenet 0.8 wrote keeps the rules of its time, so only its references are checked.
    fn check(&self, entry: &Entry, format: u64) -> Result<()> {
        match entry {
            Entry::Revision(revision) => ensure!(
                !self.revisions.contains_key(&revision.rev),
                "r{} appears twice",
                revision.rev
            ),
            Entry::Assessment(assessment) => {
                if let Some(carried) = assessment.carried_from {
                    self.revision(carried)?;
                }
            }
            Entry::Record(record) => {
                self.revision(record.rev)
                    .and_then(|revision| {
                        let assessment = (self.assessments_in(record.rev))
                            .find(|a| a.contract == record.contract && a.input == record.input)
                            .with_context(|| {
                                format!(
                                    "r{} assessed {} with other input",
                                    record.rev, record.contract
                                )
                            })?;
                        if format == log::FORMAT {
                            record::validate(record, assessment, revision, self)?;
                        }
                        Ok(())
                    })
                    .map_err(|error| {
                        anyhow!(
                            "record {} for {} is invalid: {error:#}. Tenet trusts no record that \
                             `tenet record` would refuse, and never edits the log. Move .tenet \
                             aside, run tenet check, and record the review again",
                            record.id,
                            record.contract
                        )
                    })?;
            }
        }
        Ok(())
    }

    /// Answer from earlier identical requests unless `reuse` is off; new answers are always kept.
    pub fn use_model(&mut self, identity: &str, reuse: bool) -> Result<()> {
        self.answers.load(identity, reuse)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn revisions(&self) -> impl DoubleEndedIterator<Item = &Revision> {
        self.entries.iter().filter_map(|entry| match entry {
            Entry::Revision(revision) => Some(revision),
            _ => None,
        })
    }

    pub fn revision(&self, rev: u64) -> Result<&Revision> {
        match self
            .revisions
            .get(&rev)
            .map(|&position| &self.entries[position])
        {
            Some(Entry::Revision(revision)) => Ok(revision),
            _ => bail!("r{rev} is not in the review log"),
        }
    }

    pub fn assessments(&self) -> impl DoubleEndedIterator<Item = &Assessment> {
        self.entries.iter().filter_map(|entry| match entry {
            Entry::Assessment(assessment) => Some(assessment),
            _ => None,
        })
    }

    /// The assessments one revision made, which follow it in the log.
    pub fn assessments_in(&self, rev: u64) -> impl Iterator<Item = &Assessment> {
        let start = self
            .revisions
            .get(&rev)
            .map_or(self.entries.len(), |p| p + 1);
        self.entries[start..].iter().map_while(|entry| match entry {
            Entry::Assessment(assessment) => Some(assessment),
            _ => None,
        })
    }

    pub fn records(&self) -> impl DoubleEndedIterator<Item = &Record> {
        self.entries.iter().filter_map(|entry| match entry {
            Entry::Record(record) => Some(record),
            _ => None,
        })
    }

    pub fn blob(&self, hash: &str) -> Result<Option<String>> {
        self.store
            .as_ref()
            .context("this review keeps no captured contents")?
            .blob(hash)
    }

    /// The latest record for this exact input settles the contract. Otherwise a favorable model
    /// conclusion does, unless a diff changed the contract itself: a diff check assumes the base
    /// complied with the contract, which says nothing about a new or edited requirement. Anything
    /// else waits for a record.
    pub fn state(
        &self,
        plan: &Plan,
        contract: &tenet_contracts::Contract,
        input: &str,
        model: Option<ContractStatus>,
    ) -> ContractState {
        if let Some(record) = self
            .records()
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
            ) if changed => ContractState::Pending {
                reason: Pending::ContractChanged,
            },
            Some(
                conclusion @ (ContractStatus::Verified
                | ContractStatus::Preserved
                | ContractStatus::Unaffected
                | ContractStatus::Clear),
            ) => ContractState::Settled {
                conclusion,
                record: None,
            },
            Some(ContractStatus::Failed) => ContractState::Pending {
                reason: Pending::ModelFailed,
            },
            Some(ContractStatus::Unresolved) => ContractState::Pending {
                reason: Pending::ModelUnresolved,
            },
            None => ContractState::Pending {
                reason: Pending::NotAssessed,
            },
        }
    }

    /// The latest assessment of this contract with exactly this input, from any revision.
    pub fn assessment(&self, contract: &str, input: &str) -> Option<&Assessment> {
        self.assessments()
            .rev()
            .find(|a| a.contract == contract && a.input == input)
    }

    /// Append one revision and its assessments, and keep the captured contents for later diffs.
    /// A check that repeats the latest revision's selection, inputs, and conclusions appends
    /// nothing. Returns the revision number, or `None` when this review keeps no state.
    pub fn commit(
        &mut self,
        evidence: &EvidencePacket<'_>,
        request: &CheckRequest<'_>,
        checked: &Checked,
    ) -> Result<Option<u64>> {
        let Some(store) = &self.store else {
            return Ok(None);
        };
        let plan = evidence.plan;
        let mut files = Vec::new();
        let mut positions = HashMap::new();
        let mut assessments = Vec::new();
        for (contract, concluded) in plan.contracts.iter().zip(&checked.contracts) {
            store.write_blob(&contract.document)?;
            let scoped = evidence.scoped_files(contract);
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
                store.write_blob(after)?;
            }
        }

        let selection = request.selection;
        let events = store.path(store::EVENTS);
        let mut number = 0;
        store::append_after(&events, self.end, log::FORMAT, |appended| {
            self.extend(&events, appended, self.end + appended.len() as u64)?;
            let latest = self.revisions().next_back();
            let revision = Revision {
                rev: latest.map_or(1, |latest| latest.rev + 1),
                at: now()?,
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
            if let Some(latest) = latest
                && self.repeats(latest, &revision, &assessments)
            {
                number = latest.rev;
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

    /// Whether a new revision would only repeat the latest one: the same selection and model, and
    /// for every contract the same input and conclusion.
    fn repeats(&self, latest: &Revision, revision: &Revision, assessments: &[Assessment]) -> bool {
        let same_selection = latest.same_review(revision)
            && latest.selects(&revision.paths)
            && latest.contract == revision.contract
            && latest.context == revision.context
            && latest.mode == revision.mode
            && latest.model == revision.model
            && latest.min_confidence == revision.min_confidence;
        let previous = self
            .assessments_in(latest.rev)
            .map(|a| (&a.contract, &a.input, &a.conclusion));
        same_selection
            && previous.eq(assessments
                .iter()
                .map(|a| (&a.contract, &a.input, &a.conclusion)))
    }

    fn append(&self, entry: Entry) -> Result<()> {
        let store = self.store.as_ref().context("this review keeps no state")?;
        store::append(&store.path(store::EVENTS), log::FORMAT, &[entry])
    }

    fn keep(&self, text: &str) -> Result<String> {
        match &self.store {
            Some(store) => store.write_blob(text),
            None => Ok(crate::evidence::content_hash(text)),
        }
    }
}

/// Exit status for a review's states: 4 while anything is pending, then 1 for a settled failure,
/// 3 for a settled unresolved conclusion, and 0 when every contract settled favorably.
/// Callers give cancellation (130) and execution errors (2) precedence.
pub fn exit_code<'a>(states: impl IntoIterator<Item = &'a ContractState>) -> u8 {
    let mut code = 0;
    for state in states {
        let rank = match state {
            ContractState::Pending { .. } => 4,
            ContractState::Settled {
                conclusion: ContractStatus::Failed,
                ..
            } => 1,
            ContractState::Settled {
                conclusion: ContractStatus::Unresolved,
                ..
            } => 3,
            ContractState::Settled { .. } => 0,
        };
        code = match (code, rank) {
            (4, _) | (_, 4) => 4,
            (1, _) | (_, 1) => 1,
            (3, _) | (_, 3) => 3,
            _ => 0,
        };
    }
    code
}

/// The branch and the base commit the change is measured from, for display.
fn review_label(plan: &Plan, base: Option<&str>) -> String {
    let branch = git::branch(&plan.root)
        .filter(|branch| branch != "HEAD")
        .map(|branch| format!("{branch}: "))
        .unwrap_or_default();
    match (base, &plan.base) {
        (Some(base), Some(oid)) if oid.starts_with(base) => {
            format!("{branch}changes since {}", &oid[..12])
        }
        (Some(base), Some(oid)) => format!("{branch}changes since {base} ({})", &oid[..12]),
        _ => format!("{branch}full check"),
    }
}

pub(crate) fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
