mod paths;
mod scope;

use anyhow::Result;
use serde_json::Value;

use tenet_contracts::Contract;

use super::conclusion;
use crate::{
    ConclusionReason, ContextSummary, ContractResult, ContractStatus, EvidencePacket, Mode,
    change::check_size,
};

pub(super) struct CombinedContext {
    pub state: Value,
    pub summary: ContextSummary,
    pub hash: String,
}

pub(super) enum Preparation {
    Ready(CombinedContext),
    Unresolved(Box<ContractResult>),
}

pub(super) fn prepare(evidence: &EvidencePacket<'_>, contract: &Contract) -> Preparation {
    let mut result = conclusion::decision(
        evidence,
        contract,
        ContractStatus::Unresolved,
        ConclusionReason::ContextUnavailable,
        "Combined context unavailable.",
    );
    match capture(evidence, contract) {
        Ok(context) => match check_size("combined context", context.summary.input_bytes) {
            Ok(()) => return Preparation::Ready(context),
            Err(error) => {
                result.reason_code = ConclusionReason::ContextTooLarge;
                result.reason = error.to_string();
                result.context = Some(context.summary);
                result.evidence_hash = Some(context.hash);
            }
        },
        Err(error) => {
            result.reason = format!("Combined context unavailable: {error:#}");
            if error.is::<crate::change::InputLimit>() {
                result.reason_code = ConclusionReason::ContextTooLarge;
            } else {
                result.error = Some(format!("{error:#}"));
            }
        }
    }
    Preparation::Unresolved(Box::new(result))
}

fn capture(evidence: &EvidencePacket<'_>, contract: &Contract) -> Result<CombinedContext> {
    let scope = scope::Scope::new(evidence.plan, contract);
    let mut inputs = Vec::new();
    let mut limit = None;
    for path in scope.selected.iter().chain(&scope.support) {
        match evidence.change(path) {
            Ok(change) => inputs.push(change),
            Err(error) if error.is::<crate::change::InputLimit>() => {
                limit.get_or_insert(error);
            }
            Err(error) => return Err(error),
        }
    }
    if let Some(error) = limit {
        return Err(error);
    }
    let files: Vec<_> = inputs.iter().map(|change| change.current_input()).collect();
    let changes = if evidence.plan.mode == Mode::Diff {
        &inputs[..scope.selected.len()]
    } else {
        &[]
    };
    let mut fields = serde_json::Map::from_iter([
        ("contract".into(), serde_json::json!(contract.body)),
        ("requested_scope".into(), serde_json::json!(scope.requested)),
        ("selected_files".into(), serde_json::json!(scope.selected)),
        ("support_files".into(), serde_json::json!(scope.support)),
        ("files".into(), serde_json::json!(files)),
        ("changes".into(), serde_json::json!(changes)),
    ]);
    fields.extend(paths::metadata(&scope)?);
    let state = Value::Object(fields);
    let encoded = serde_json::to_vec(&state)?;
    Ok(CombinedContext {
        state,
        summary: scope.summary(encoded.len()),
        hash: blake3::hash(&encoded).to_hex().to_string(),
    })
}
