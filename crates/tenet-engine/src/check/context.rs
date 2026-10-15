mod paths;
pub(crate) mod scope;

use anyhow::Result;
use serde_json::Value;

use tenet_contracts::Contract;

use crate::{
    ConclusionReason, ContextSummary, EvidencePacket, MAX_FILE_BYTES, Mode,
    change::{Unreadable, omitted},
};

pub(super) struct CombinedContext {
    pub state: Value,
    pub summary: ContextSummary,
    pub hash: String,
}

/// Why the combined assessment cannot run for a contract.
pub(super) struct Unavailable {
    pub reason_code: ConclusionReason,
    pub reason: String,
    pub error: Option<String>,
    pub context: Option<Box<CombinedContext>>,
}

impl EvidencePacket<'_> {
    /// What a contract's combined assessment would receive, without sending it.
    pub fn combined_summary(&self, contract: &Contract) -> Result<ContextSummary> {
        Ok(capture(self, contract)?.summary)
    }
}

pub(super) fn prepare(
    evidence: &EvidencePacket<'_>,
    contract: &Contract,
) -> Result<CombinedContext, Unavailable> {
    match capture(evidence, contract) {
        Ok(context) if context.summary.fits() => Ok(context),
        Ok(context) => Err(Unavailable {
            reason_code: ConclusionReason::ContextTooLarge,
            reason: format!(
                "Combined context is {} bytes after leaving out what each patch shows; limit is {MAX_FILE_BYTES} bytes.",
                context.summary.input_bytes
            ),
            error: None,
            context: Some(Box::new(context)),
        }),
        Err(error) => Err(Unavailable {
            reason_code: ConclusionReason::ContextUnavailable,
            reason: format!("Combined context unavailable: {error:#}"),
            error: Some(format!("{error:#}")),
            context: None,
        }),
    }
}

/// Assemble the contract's complete scoped evidence. When it exceeds the input limit, leave out
/// prior contents, then inventory detail, then the largest resulting contents, until it fits.
/// Text is left out only where the file's patch remains, and every omission is marked in the
/// input and listed in the summary.
fn capture(evidence: &EvidencePacket<'_>, contract: &Contract) -> Result<CombinedContext> {
    let scope = scope::Scope::new(evidence.plan, contract);
    let mut files = Vec::new();
    let mut changes = Vec::new();
    let mut patched = std::collections::BTreeSet::new();
    for (index, path) in scope.selected.iter().chain(&scope.support).enumerate() {
        match evidence.change(path) {
            Ok(change) => {
                files.push(change.current_input());
                if evidence.plan.mode == Mode::Diff && index < scope.selected.len() {
                    changes.push(serde_json::to_value(change)?);
                    if !change.patch.is_empty() {
                        patched.insert(change.path.clone());
                    }
                }
            }
            Err(error) => match error.downcast_ref::<Unreadable>() {
                Some(unreadable) => files.push(serde_json::json!({
                    "path": path,
                    "contents": omitted(&unreadable.to_string()),
                })),
                None => return Err(error),
            },
        }
    }
    let mut summary = scope.summary(0);
    let mut inventory = paths::metadata(&scope)?;
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
        let befores: Vec<_> = changes
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
                        .is_some_and(|path| patched.contains(std::path::Path::new(path)))
            })
            .max_by_key(|file| file["contents"].as_str().map_or(0, str::len));
        if encoded.len() <= MAX_FILE_BYTES {
            // Fits as assembled.
        } else if !befores.is_empty() {
            for before in befores {
                *before = omitted("too large; use the patch");
            }
            summary.before_omitted = true;
            continue;
        } else if let Some(depth) = paths::coarser(&scope, counted_depth) {
            inventory = paths::counts(&scope, depth)?;
            counted_depth = Some(depth);
            summary.inventory_counted = true;
            continue;
        } else if let Some(file) = largest {
            file["contents"] = omitted("too large; use the patch");
            summary
                .contents_omitted
                .push(serde_json::from_value(file["path"].clone())?);
            continue;
        }
        return Ok(CombinedContext {
            state,
            summary,
            hash: blake3::hash(&encoded).to_hex().to_string(),
        });
    }
}
