use alloc::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The conclusion a contract reaches for one review.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractStatus {
    Clear,
    Failed,
    Preserved,
    Unaffected,
    Unresolved,
    Verified,
}

impl ContractStatus {
    /// Return the conclusion's name as reports and records spell it.
    #[must_use]
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Preserved => "preserved",
            Self::Clear => "clear",
            Self::Unaffected => "unaffected",
            Self::Failed => "failed",
            Self::Unresolved => "unresolved",
        }
    }

    /// Read a conclusion from its name, or return `None` for an unknown name.
    #[must_use]
    #[inline]
    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Verified,
            Self::Preserved,
            Self::Clear,
            Self::Unaffected,
            Self::Failed,
            Self::Unresolved,
        ]
        .into_iter()
        .find(|status| status.as_str() == value)
    }
}

/// Whether a check read a contract's whole scope or only a diff of it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestedScope {
    FullContract,
    SelectedSubjects,
}

impl RequestedScope {
    /// What a conclusion about this scope covers, in prose.
    #[must_use]
    #[inline]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::FullContract => "the full contract",
            Self::SelectedSubjects => "selected subjects",
        }
    }
}

/// A contract's conclusion and the results that support it.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Clone, Debug, Serialize)]
pub struct ContractResult {
    #[serde(rename = "assessment")]
    pub status: ContractStatus,
    pub scope: RequestedScope,
    /// Identifies the contract document and every scoped fact the assessment covered.
    pub input: String,
    /// The model's probability that the requested scope violates the contract.
    pub violation: Option<f64>,
    pub reason: String,
    pub reason_code: ConclusionReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextSummary>,
    #[serde(rename = "model_assessment", skip_serializing_if = "Option::is_none")]
    pub assessment: Option<ev_grep_core::Assessment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing: Option<ev_grep_core::Routing>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Why the combined assessment's request returned no answer, when it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<crate::RequestFailure>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub conflicting_files: Vec<String>,
    pub request: Option<ev_grep_core::RequestInfo>,
    /// The combined answer came from the answer cache, or `check --no-model` reused an earlier
    /// check's conclusion for the same input; no request was sent.
    pub cached: bool,
}

/// Why a contract reached its conclusion, as a stable code.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConclusionReason {
    AssessmentFailed,
    Compliance,
    ConflictingAssessments,
    ContextTooLarge,
    ContextUnavailable,
    FileChecksIncomplete,
    /// File checks found a violation the combined assessment did not confirm or could not run.
    FileViolation,
    ModelUncertain,
    NoRelevantChanges,
    RequestBudget,
    SelectedSubjectsClear,
    Violation,
}

/// The combined context a contract's check read, by size and paths.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Clone, Debug, Serialize)]
pub struct ContextSummary {
    pub input_bytes: usize,
    pub limit_bytes: usize,
    pub included_files: usize,
    pub selected_files: Vec<PathBuf>,
    pub support_files: Vec<PathBuf>,
    /// Support files supplied as excerpts, and selected files whose excerpt stood in for contents
    /// left out to fit the limit, with the inclusive line ranges supplied.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub excerpts: BTreeMap<PathBuf, Vec<[usize; 2]>>,
    /// Scoped inventory files whose contents were not supplied.
    pub omitted_files: usize,
    /// Prior contents left out of every change to fit the limit; patches remain.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub before_omitted: bool,
    /// Inventory paths summarized as per-directory file counts to fit the limit.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub inventory_counted: bool,
    /// Changed files whose resulting contents were left out to fit the limit; patches and any
    /// excerpts remain.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub contents_omitted: Vec<PathBuf>,
}

impl ContextSummary {
    /// Whether the combined input fits its limit, so the combined assessment can run.
    #[must_use]
    #[inline]
    pub const fn is_fitting(&self) -> bool {
        self.input_bytes <= self.limit_bytes
    }

    /// Whether anything was left out of a combined input that fit the limit.
    #[must_use]
    #[inline]
    pub const fn is_reduced(&self) -> bool {
        self.is_fitting()
            && (self.before_omitted || self.inventory_counted || !self.contents_omitted.is_empty())
    }
}

/// Where a contract stands in its review once records and model conclusions are combined.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ContractState {
    /// The result is not final until a check or a record settles it.
    Pending { reason: Pending },
    /// A favorable model conclusion, or a record bound to the current input.
    Settled {
        conclusion: ContractStatus,
        /// The record that settled the contract; absent when the model's conclusion did.
        record: Option<String>,
    },
}

impl ContractState {
    /// Return whether the contract still waits for a decision.
    #[must_use]
    #[inline]
    pub const fn is_pending(&self) -> bool {
        matches!(self, Self::Pending { .. })
    }
}

/// Why a contract is still pending.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Pending {
    /// The diff adds or edits the contract itself, so a diff check's favorable conclusion, which
    /// assumes the base complied, does not settle it; a full check informs the record that does.
    ContractChanged,
    /// Scoped files or the contract changed after the last check.
    InputsChanged,
    /// The model reports a violation; an agent confirms or disputes it with a record.
    ModelFailed,
    /// The model could not decide, or the assessment could not complete.
    ModelUnresolved,
    /// Captured without a model, and no earlier assessment had the same input.
    NotAssessed,
    /// The contract entered the review's scope and has not been checked.
    NotChecked,
}
