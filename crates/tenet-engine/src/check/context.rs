mod paths;
pub(crate) mod scope;

use alloc::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tenet_contracts::Contract;

use crate::{
    ConclusionReason,
    ContextSummary,
    EvidencePacket,
    MAX_FILE_BYTES,
    Mode,
    change::describe_omitted,
    evidence::ChangeError,
    plan::ExcerptError,
    report::Causes,
};

/// Why a contract's combined context cannot be assembled.
#[derive(Debug, thiserror::Error)]
pub enum CombinedError {
    #[error(transparent)]
    Change(#[from] ChangeError),
    #[error(transparent)]
    Encode(#[from] serde_json::Error),
    #[error(transparent)]
    Excerpt(#[from] ExcerptError),
    #[error("inventory path is not UTF-8")]
    NotUtf8Path,
}

pub(super) struct CombinedContext {
    pub hash: String,
    pub state: Value,
    pub summary: ContextSummary,
}

/// Why the combined assessment cannot run for a contract.
pub(super) struct Unavailable {
    pub context: Option<Box<CombinedContext>>,
    pub error: Option<String>,
    pub failure: Option<crate::RequestFailure>,
    pub reason: String,
    pub reason_code: ConclusionReason,
}

impl EvidencePacket<'_> {
    /// What a contract's combined assessment would receive, without sending it.
    #[inline]
    pub fn summarize_combined(&self, contract: &Contract) -> Result<ContextSummary, CombinedError> {
        Ok(capture(self, contract)?.summary)
    }
}

pub(super) fn prepare(
    evidence: &EvidencePacket<'_>,
    contract: &Contract,
) -> Result<CombinedContext, Unavailable> {
    match capture(evidence, contract) {
        Ok(context) if context.summary.is_fitting() => Ok(context),
        Ok(context) => {
            let reason = format!(
                "Combined context is {} bytes after leaving out what each patch shows; limit is \
                 {MAX_FILE_BYTES} bytes.",
                context.summary.input_bytes
            );
            Err(Unavailable {
                context: Some(Box::new(context)),
                error: None,
                failure: None,
                reason,
                reason_code: ConclusionReason::ContextTooLarge,
            })
        },
        Err(error) => {
            Err(Unavailable {
                context: None,
                error: Some(Causes(&error).to_string()),
                failure: None,
                reason: format!("Combined context unavailable: {:#}", Causes(&error)),
                reason_code: ConclusionReason::ContextUnavailable,
            })
        },
    }
}

/// Assemble the contract's complete scoped evidence.
///
/// When it exceeds the input limit, leave out prior contents, then inventory detail, then the
/// largest resulting contents, until it fits. Text is left out only where the file's patch remains,
/// and every omission is marked in the input and listed in the summary. Lines of a file named with
/// `--context` stand in for its resulting contents, and are left out last.
#[expect(
    clippy::excessive_nesting,
    reason = "Each level is a loop, match or closure over one parsed structure, and flattening it \
              would split one decision."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
#[expect(
    clippy::else_if_without_else,
    reason = "`needless_else` rejects the empty `else` this lint asks for."
)]
fn capture(
    evidence: &EvidencePacket<'_>,
    contract: &Contract,
) -> Result<CombinedContext, CombinedError> {
    let scope = scope::Scope::new(evidence.plan, contract);
    let mut files = Vec::new();
    let mut changes = Vec::new();
    let mut patched_paths = BTreeSet::new();
    for (index, path) in scope.selected.iter().chain(&scope.support).enumerate() {
        let selected = index < scope.selected.len();
        match evidence.change(path) {
            Ok(change) => {
                files.push(if selected {
                    change.describe_current_input()
                } else {
                    evidence.plan.build_context_input(change)?
                });
                if evidence.plan.mode == Mode::Diff && selected {
                    changes.push(serde_json::to_value(change)?);
                    if !change.patch.is_empty() {
                        let _: bool = patched_paths.insert(change.path.clone());
                    }
                }
            },
            Err(ChangeError::Unreadable { reason, .. }) => {
                files.push(serde_json::json!({
                    "path": path,
                    "contents": describe_omitted(&reason.to_string()),
                }));
            },
            Err(error @ (ChangeError::Uncaptured { .. } | ChangeError::NotCaptured(_))) => {
                return Err(error.into());
            },
        }
    }
    let mut summary = scope.summarize(evidence.plan, 0);
    let mut inventory = paths::collect_metadata(&scope)?;
    let mut counted_depth = None;
    loop {
        let mut fields = serde_json::Map::from_iter([
            ("contract".into(), serde_json::json!(contract.body)),
            ("requested_scope".into(), serde_json::json!(scope.requested)),
            ("selected_files".into(), serde_json::json!(scope.selected)),
            ("support_files".into(), serde_json::json!(scope.support)),
            ("files".into(), serde_json::json!(files)),
            ("changes".into(), serde_json::json!(changes)),
        ]);
        fields.extend(inventory.clone());
        let state = Value::Object(fields);
        let encoded = serde_json::to_vec(&state)?;
        summary.input_bytes = encoded.len();
        let patched_before: Vec<_> = changes
            .iter_mut()
            .filter(|change| {
                change["patch"]
                    .as_str()
                    .is_some_and(|patch| !patch.is_empty())
            })
            .map(|change| &mut change["before"])
            .filter(|before| before.is_string())
            .collect();
        let largest = files
            .iter_mut()
            .filter(|file| {
                file["contents"].is_string()
                    && file["path"]
                        .as_str()
                        .is_some_and(|path| patched_paths.contains(Path::new(path)))
            })
            .max_by_key(|file| file["contents"].as_str().map_or(0, str::len));
        if encoded.len() <= MAX_FILE_BYTES {
            // Fits as assembled.
        } else if !patched_before.is_empty() {
            for before in patched_before {
                *before = describe_omitted("too large; use the patch");
            }
            summary.before_omitted = true;
            continue;
        } else if let Some(depth) = paths::find_coarser(&scope, counted_depth) {
            inventory = paths::count_paths(&scope, depth)?;
            counted_depth = Some(depth);
            summary.inventory_counted = true;
            continue;
        } else if let Some(file) = largest {
            let path: PathBuf = serde_json::from_value(file["path"].clone())?;
            file["contents"] = match evidence.plan.excerpts.get(&path) {
                Some(excerpt) => {
                    drop(summary.excerpts.insert(path.clone(), excerpt.lines.clone()));
                    excerpt.contents()?
                },
                None => describe_omitted("too large; use the patch"),
            };
            summary.contents_omitted.push(path);
            continue;
        } else if let Some((file, path)) = files.iter_mut().find_map(|file| {
            let path = PathBuf::from(file["path"].as_str()?);
            (file["contents"].get("excerpt").is_some() && patched_paths.contains(&path))
                .then_some((file, path))
        }) {
            // Even the named lines of a changed file do not fit; its patch remains.
            file["contents"] = describe_omitted("too large; use the patch");
            drop(summary.excerpts.remove(&path));
            continue;
        }
        return Ok(CombinedContext {
            hash: blake3::hash(&encoded).to_hex().to_string(),
            state,
            summary,
        });
    }
}
