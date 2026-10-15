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
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde_json::Value;

pub(crate) use answers::Answers;
pub use current::{Current, CurrentContract, current};
pub use log::{
    Assessment, Conclusion, Entry, FileResult, Record, Revision, Role, ScopedFile, Span,
};
pub use record::{RecordRequest, SpanRequest, conclusions, record};
use store::Store;

use crate::{ContractState, ContractStatus, EvidencePacket, Pending, Selection, Summary, git};

pub struct Review {
    store: Option<Store>,
    /// The log in the order it was written.
    entries: Vec<Entry>,
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
    pub summary: Summary,
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

    pub fn pending(&self) -> Vec<String> {
        self.contracts
            .iter()
            .filter(|c| c.state.is_pending())
            .map(|c| c.contract.clone())
            .collect()
    }
}

/// What a check was asked to assess, kept so later commands can capture the same inputs again.
pub struct CheckRequest<'a> {
    pub selection: &'a Selection,
    pub context: &'a [PathBuf],
    /// The provider and model that answered; absent for `check --no-model`.
    pub model: Option<&'a str>,
}

impl Review {
    /// Open the project's review state, creating `.tenet/` when needed.
    pub fn open(root: &Path) -> Result<Self> {
        Self::load(Store::create(root)?)
    }

    /// Open existing review state without writing anything.
    pub fn existing(root: &Path) -> Result<Self> {
        let store = Store::existing(root).with_context(|| {
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
            answers: Answers::new(None),
        }
    }

    fn load(store: Store) -> Result<Self> {
        let mut review = Self {
            answers: Answers::new(Some(store.path(store::ANSWERS))),
            entries: Vec::new(),
            store: None,
        };
        for value in store::read(&store.path(store::EVENTS))? {
            review
                .entries
                .push(serde_json::from_value(value).context("invalid review event")?);
        }
        review.store = Some(store);
        Ok(review)
    }

    /// Answer from earlier identical requests unless `reuse` is off; new answers are always kept.
    pub fn use_model(&mut self, identity: &str, reuse: bool) -> Result<()> {
        self.answers.load(identity, reuse)
    }

    pub fn is_persistent(&self) -> bool {
        self.store.is_some()
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

    pub fn assessments(&self) -> impl DoubleEndedIterator<Item = &Assessment> {
        self.entries.iter().filter_map(|entry| match entry {
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
    /// conclusion does, and anything else waits for a record.
    pub fn state(
        &self,
        contract: &str,
        input: &str,
        model: Option<ContractStatus>,
    ) -> ContractState {
        if let Some(record) = self
            .records()
            .rev()
            .find(|record| record.contract == contract && record.input == input)
        {
            return ContractState::Settled {
                conclusion: record.conclusion,
                record: Some(record.id.clone()),
            };
        }
        match model {
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

    /// Reuse earlier conclusions for unchanged inputs without asking a model.
    pub fn carry(&self, evidence: &EvidencePacket<'_>) -> Checked {
        let contracts = evidence
            .plan
            .contracts
            .iter()
            .map(|contract| {
                let input = evidence.input_key(contract);
                let earlier = self.assessments().rev().find(|a| {
                    a.contract == contract.name && a.input == input && a.conclusion.is_some()
                });
                let conclusion = earlier.and_then(|a| a.conclusion.clone());
                let state = self.state(
                    &contract.name,
                    &input,
                    conclusion.as_ref().map(|c| c.status),
                );
                Concluded {
                    contract: contract.name.clone(),
                    scope: evidence.plan.requested_scope(contract),
                    conclusion,
                    carried_from: earlier.map(|a| a.rev),
                    results: earlier.map(|a| a.results.clone()).unwrap_or_default(),
                    input,
                    state,
                }
            })
            .collect();
        Checked {
            summary: Summary::default(),
            contracts,
        }
    }

    /// Append one revision and its assessments, and keep the captured contents for later diffs.
    /// A check that repeats the latest revision's selection, inputs, and conclusions appends
    /// nothing. Returns the revision number, or `None` when this review keeps no state.
    pub fn commit(
        &self,
        evidence: &EvidencePacket<'_>,
        request: &CheckRequest<'_>,
        checked: &Checked,
    ) -> Result<Option<u64>> {
        let Some(store) = &self.store else {
            return Ok(None);
        };
        let plan = evidence.plan;
        let mut assessments = Vec::new();
        for concluded in &checked.contracts {
            let Some(contract) = plan.contracts.iter().find(|c| c.name == concluded.contract)
            else {
                continue;
            };
            store.write_blob(&contract.document)?;
            let files = evidence.scoped_files(contract);
            for file in files.iter().filter(|file| file.after.is_some()) {
                if let Some(after) = &evidence.change(&file.path)?.after {
                    store.write_blob(after)?;
                }
            }
            assessments.push((concluded, &contract.hash, files));
        }
        let selection = request.selection;
        let mut revision_number = 0;
        store::append_after(&store.path(store::EVENTS), |existing| {
            let entries = existing
                .iter()
                .map(|value| serde_json::from_value::<Entry>(value.clone()))
                .collect::<Result<Vec<_>, _>>()
                .context("invalid review event")?;
            let latest = entries.iter().rev().find_map(|entry| match entry {
                Entry::Revision(revision) => Some(revision),
                _ => None,
            });
            let revision = Revision {
                rev: latest.map_or(1, |latest| latest.rev + 1),
                at: now()?,
                review: review_label(&plan.root, selection.base.as_deref()),
                base: selection.base.clone(),
                paths: selection.paths.clone(),
                contract: selection.contract.clone(),
                context: request.context.to_vec(),
                mode: plan.mode,
                base_oid: plan.base.clone(),
                head_oid: plan.head.clone(),
                model: request.model.map(str::to_owned),
                tenet: env!("CARGO_PKG_VERSION").into(),
            };
            if let Some(latest) = latest
                && repeats(latest, &revision, &entries, &assessments)
            {
                revision_number = latest.rev;
                return Ok(Vec::new());
            }
            revision_number = revision.rev;
            let mut lines = vec![serde_json::to_value(Entry::Revision(revision))?];
            for (concluded, contract_hash, files) in assessments {
                lines.push(serde_json::to_value(Entry::Assessment(Assessment {
                    rev: revision_number,
                    contract: concluded.contract.clone(),
                    contract_hash: contract_hash.clone(),
                    input: concluded.input.clone(),
                    scope: concluded.scope,
                    conclusion: concluded.conclusion.clone(),
                    carried_from: concluded.carried_from,
                    files,
                    results: concluded.results.clone(),
                }))?);
            }
            Ok(lines)
        })?;
        Ok(Some(revision_number))
    }

    fn append(&self, entry: Entry) -> Result<()> {
        let store = self.store.as_ref().context("this review keeps no state")?;
        let line: Value = serde_json::to_value(entry)?;
        store::append(&store.path(store::EVENTS), vec![line])
    }

    fn keep(&self, text: &str) -> Result<String> {
        match &self.store {
            Some(store) => store.write_blob(text),
            None => Ok(crate::evidence::content_hash(text)),
        }
    }
}

/// Whether a new revision would only repeat the latest one: the same selection and model, and for
/// every contract the same input and conclusion.
fn repeats(
    latest: &Revision,
    revision: &Revision,
    entries: &[Entry],
    assessments: &[(&Concluded, &String, Vec<ScopedFile>)],
) -> bool {
    let same_selection = latest.same_review(revision)
        && latest.contract == revision.contract
        && latest.context == revision.context
        && latest.mode == revision.mode
        && latest.model == revision.model;
    let previous: Vec<_> = entries
        .iter()
        .filter_map(|entry| match entry {
            Entry::Assessment(a) if a.rev == latest.rev => {
                Some((a.contract.as_str(), a.input.as_str(), a.conclusion.as_ref()))
            }
            _ => None,
        })
        .collect();
    let current: Vec<_> = assessments
        .iter()
        .map(|(c, ..)| (c.contract.as_str(), c.input.as_str(), c.conclusion.as_ref()))
        .collect();
    same_selection && previous == current
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

fn review_label(root: &Path, base: Option<&str>) -> String {
    let branch = git::branch(root).unwrap_or_else(|| "worktree".into());
    match base {
        Some(base) => format!("{branch}…{base}"),
        None => format!("{branch} (full)"),
    }
}

pub(crate) fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
