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
            result.reason = format!("Combined context unavailable: {error}");
            result.error = Some(error.to_string());
        }
    }
    Preparation::Unresolved(Box::new(result))
}

fn capture(evidence: &EvidencePacket<'_>, contract: &Contract) -> Result<CombinedContext> {
    let scope = scope::Scope::new(evidence.plan, contract);
    let changes = scope
        .selected
        .iter()
        .map(|path| evidence.change(path))
        .collect::<Result<Vec<_>>>()?;
    let mut files: Vec<_> = changes
        .iter()
        .map(|change| change.current_input())
        .collect();
    for path in &scope.support {
        files.push(evidence.change(path)?.current_input());
    }
    let changes = if evidence.plan.mode == Mode::Diff {
        changes
    } else {
        Vec::new()
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
