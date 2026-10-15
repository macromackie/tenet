//! Entries in `.tenet/events.jsonl`, the review's only source of truth.

use std::{
    collections::{BTreeSet, HashMap},
    path::{Component, Path, PathBuf},
};

use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::{ConclusionReason, ContractStatus, Mode, Omitted, RequestedScope, Status};

/// The format `check` and `record` write. Each revision lists its captured files once, and its
/// assessments refer to them by position.
pub(super) const FORMAT: u64 = 2;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Entry {
    Revision(Revision),
    Assessment(Assessment),
    Record(Record),
}

/// One `tenet check`. Its selection lets later commands recapture exactly what it assessed.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Revision {
    pub rev: u64,
    /// Unix seconds.
    pub at: u64,
    /// The branch and base this revision belongs to, for display.
    pub review: String,
    pub base: Option<String>,
    pub paths: Vec<PathBuf>,
    pub contract: Option<String>,
    pub context: Vec<crate::SpanRequest>,
    pub mode: Mode,
    pub base_oid: Option<String>,
    pub head_oid: Option<String>,
    /// The provider and model that answered; absent for `check --no-model`.
    pub model: Option<String>,
    /// The confidence below which the model's answers were routed to unresolved; absent for
    /// `check --no-model` and for revisions written before Tenet recorded it.
    pub min_confidence: Option<f64>,
    pub tenet: String,
    /// Every file this revision's assessments cover, each once. Format 1 listed them in each
    /// assessment instead.
    #[serde(default)]
    pub files: Vec<CapturedFile>,
}

impl Revision {
    /// Revisions against the same base commit review the same change, whichever paths and
    /// contracts they selected. Full checks have no base and form one review.
    pub fn same_review(&self, other: &Self) -> bool {
        self.base_oid == other.base_oid
    }

    /// Whether this revision's path selection includes everything `earlier` selected, so its
    /// assessment of a contract replaces `earlier`'s. No paths select everything.
    pub fn covers(&self, earlier: &Self) -> bool {
        let selected = selectors(&self.paths);
        let earlier = selectors(&earlier.paths);
        selected.is_empty()
            || (!earlier.is_empty()
                && earlier
                    .iter()
                    .all(|path| selected.iter().any(|selector| path.starts_with(selector))))
    }

    /// Whether these are the revision's selected paths, however they were spelled.
    pub fn selects(&self, paths: &[PathBuf]) -> bool {
        selectors(&self.paths) == selectors(paths)
    }

    /// The files one of this revision's assessments covers: its selected files, then its
    /// supporting files.
    pub fn files<'a>(
        &'a self,
        assessment: &'a Assessment,
    ) -> impl Iterator<Item = (Role, &'a CapturedFile)> {
        let selected = assessment.selected.iter().map(|&i| (Role::Selected, i));
        let support = assessment.support.iter().map(|&i| (Role::Support, i));
        selected
            .chain(support)
            .map(|(role, i)| (role, &self.files[i]))
    }
}

fn selectors(paths: &[PathBuf]) -> BTreeSet<PathBuf> {
    paths
        .iter()
        .map(|path| {
            path.components()
                .filter(|part| *part != Component::CurDir)
                .collect::<PathBuf>()
        })
        .filter(|path| path != Path::new(""))
        .collect()
}

/// One contract's assessment in one revision, bound to its exact input.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Assessment {
    pub rev: u64,
    pub contract: String,
    /// The contract document's hash; its text is kept in `.tenet/blobs/`.
    pub contract_hash: String,
    pub input: String,
    pub scope: RequestedScope,
    /// The model's conclusion; absent when the contract was captured without one.
    pub conclusion: Option<Conclusion>,
    /// The revision whose model conclusion for this same input `check --no-model` reused.
    pub carried_from: Option<u64>,
    /// Positions in the revision's `files` of the files this contract selected.
    pub selected: Vec<usize>,
    /// Positions of the `--context` files that supported them.
    pub support: Vec<usize>,
    pub results: Vec<FileResult>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Conclusion {
    pub status: ContractStatus,
    pub reason_code: ConclusionReason,
    pub reason: String,
    /// The model's probability of a violation in the requested scope.
    pub violation: Option<f64>,
    /// The combined assessment's raw answer before confidence routing; absent when it did not run
    /// and in revisions written before Tenet recorded it.
    pub answer: Option<Answer>,
    pub error: Option<String>,
    pub conflicting_files: Vec<String>,
}

/// A model's raw choice and its confidence in it.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct Answer {
    pub choice: ev_grep_core::Outcome,
    pub confidence: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Selected,
    Support,
}

/// A captured file's identity. Contents live in `.tenet/blobs/` under these hashes.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct CapturedFile {
    pub path: PathBuf,
    pub previous_path: Option<PathBuf>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub error: Option<String>,
    /// In a diff, the resulting lines the change wrote, as in `Change::changed`, so a record can
    /// be held to the change. Input keys leave them out; `before` and `after` determine them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<[usize; 2]>,
    /// For a file named with `--context PATH:START-END`, the resulting lines the check supplied,
    /// so the input key holds them and a record cites a supporting file only within them. Empty
    /// for a whole file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excerpt: Vec<[usize; 2]>,
}

impl CapturedFile {
    pub fn deleted(&self) -> bool {
        self.before.is_some() && self.after.is_none()
    }

    /// Whether the change touched this file: wrote lines in it or deleted it.
    pub fn is_changed(&self) -> bool {
        !self.changed.is_empty() || self.deleted()
    }

    /// Whether the change left this file without text, by deleting or emptying it, so no line
    /// shows the change.
    fn cleared(&self) -> bool {
        self.before.is_some()
            && (self.after.as_deref())
                .is_none_or(|after| after == crate::evidence::content_hash(""))
    }

    /// Whether a citation of this file, with `lines` or whole, overlaps or adjoins a line the
    /// change wrote. Only a deleted or emptied file is cited whole.
    pub fn cites_change(&self, lines: Option<[usize; 2]>) -> bool {
        match lines {
            None => self.cleared(),
            Some([start, end]) => {
                (self.changed.iter()).any(|&[first, last]| start <= last + 1 && first <= end + 1)
            }
        }
    }

    /// The citations that show this file's change: each changed range, or the cleared file.
    pub fn change_citations(&self) -> Vec<String> {
        let path = self.path.display();
        if self.cleared() {
            return vec![format!("{path} (whole file)")];
        }
        (self.changed.iter())
            .map(|[start, end]| {
                if start == end {
                    format!("{path}:{start}")
                } else {
                    format!("{path}:{start}-{end}")
                }
            })
            .collect()
    }
}

/// A captured file in one contract's scope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopedFile {
    pub role: Role,
    #[serde(flatten)]
    pub file: CapturedFile,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileResult {
    pub path: String,
    pub status: Status,
    /// The model's probability that this file violates the contract.
    pub violation: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omitted: Vec<Omitted>,
}

/// An agent's or person's conclusion for one contract input. Never edited; a later record for
/// the same input supersedes it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Record {
    pub id: String,
    pub at: u64,
    pub rev: u64,
    pub contract: String,
    pub input: String,
    pub conclusion: ContractStatus,
    pub evidence: Vec<Span>,
    pub message: String,
    /// The model's conclusion for this input when the record was made.
    pub model: Option<ContractStatus>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Span {
    pub path: PathBuf,
    /// Inclusive, 1-based; absent when the evidence is the whole file.
    pub lines: Option<[usize; 2]>,
    pub blob: String,
}

/// Parse one line of the log. An assessment must follow `revision`, its revision, and refer only to
/// files it captured. Format 1, which Tenet 0.8 wrote, listed every assessment's files in full;
/// those move into `revision`, so records made with Tenet 0.8 keep settling their inputs, and
/// `moved` remembers where each went.
pub(super) fn parse(
    format: u64,
    line: &str,
    revision: Option<&mut Revision>,
    moved: &mut HashMap<CapturedFile, usize>,
) -> Result<Entry> {
    // The type selects the struct to parse. An internally tagged enum would buffer the line first,
    // which loses the arbitrary-precision numbers serde_json is built with here.
    #[derive(Deserialize)]
    struct Kind {
        #[serde(rename = "type")]
        kind: String,
    }

    #[derive(Deserialize)]
    struct Assessment1 {
        rev: u64,
        contract: String,
        contract_hash: String,
        input: String,
        scope: RequestedScope,
        conclusion: Option<Conclusion>,
        carried_from: Option<u64>,
        files: Vec<ScopedFile>,
        results: Vec<FileResult>,
    }

    let Kind { kind } = serde_json::from_str(line)?;
    Ok(match (format, kind.as_str()) {
        (1 | FORMAT, "revision") => {
            moved.clear();
            Entry::Revision(serde_json::from_str(line)?)
        }
        (1 | FORMAT, "record") => Entry::Record(serde_json::from_str(line)?),
        (1 | FORMAT, "assessment") => {
            let (mut assessment, files) = if format == 1 {
                let old: Assessment1 = serde_json::from_str(line)?;
                let assessment = Assessment {
                    rev: old.rev,
                    contract: old.contract,
                    contract_hash: old.contract_hash,
                    input: old.input,
                    scope: old.scope,
                    conclusion: old.conclusion,
                    carried_from: old.carried_from,
                    selected: Vec::new(),
                    support: Vec::new(),
                    results: old.results,
                };
                (assessment, old.files)
            } else {
                (serde_json::from_str(line)?, Vec::new())
            };
            let (rev, contract) = (assessment.rev, &assessment.contract);
            let Some(revision) = revision.filter(|revision| revision.rev == rev) else {
                bail!("the assessment of {contract} does not follow r{rev}");
            };
            if format == 1 {
                (assessment.selected, assessment.support) =
                    add_files(&mut revision.files, moved, files);
            }
            ensure!(
                (assessment.selected.iter())
                    .chain(&assessment.support)
                    .all(|&position| position < revision.files.len()),
                "the assessment of {contract} names a file r{rev} did not capture"
            );
            Entry::Assessment(assessment)
        }
        (1 | FORMAT, kind) => bail!("unknown entry type {kind}"),
        _ => bail!("format {format} is not one this Tenet reads; upgrade Tenet"),
    })
}

/// Add one assessment's scoped files to a revision's `files`, keeping one copy of each, and return
/// the positions of its selected files and of its supporting files. `positions` remembers where
/// each file already is.
pub(super) fn add_files(
    files: &mut Vec<CapturedFile>,
    positions: &mut HashMap<CapturedFile, usize>,
    scoped: Vec<ScopedFile>,
) -> (Vec<usize>, Vec<usize>) {
    let (mut selected, mut support) = (Vec::new(), Vec::new());
    for ScopedFile { role, file } in scoped {
        let position = *positions.entry(file.clone()).or_insert_with(|| {
            files.push(file);
            files.len() - 1
        });
        match role {
            Role::Selected => selected.push(position),
            Role::Support => support.push(position),
        }
    }
    (selected, support)
}
