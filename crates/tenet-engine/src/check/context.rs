mod paths;
mod scope;

use anyhow::Result;
use serde_json::Value;

use super::conclusion::Completion;
use crate::{ContextSummary, Mode, change::check_size};

pub(super) struct CombinedContext {
    pub state: Value,
    pub summary: ContextSummary,
    pub hash: String,
}

impl Completion<'_> {
    pub(super) fn evidence(&self) -> Result<CombinedContext> {
        let scope = scope::Scope::new(self.evidence.plan, self.contract);
        let changes = scope
            .selected
            .iter()
            .map(|path| self.evidence.change(path))
            .collect::<Result<Vec<_>>>()?;
        let mut files: Vec<_> = changes
            .iter()
            .map(|change| change.current_input())
            .collect();
        for path in &scope.support {
            files.push(self.evidence.change(path)?.current_input());
        }
        let changes = if self.evidence.plan.mode == Mode::Diff {
            changes
        } else {
            Vec::new()
        };
        let mut fields = serde_json::Map::from_iter([
            ("contract".into(), serde_json::json!(self.contract.body)),
            ("requested_scope".into(), serde_json::json!(scope.requested)),
            ("selected_files".into(), serde_json::json!(scope.selected)),
            ("support_files".into(), serde_json::json!(scope.support)),
            ("files".into(), serde_json::json!(files)),
            ("changes".into(), serde_json::json!(changes)),
        ]);
        fields.extend(paths::metadata(&scope)?);
        let state = Value::Object(fields);
        let encoded = serde_json::to_vec(&state)?;
        check_size("combined context", encoded.len())?;
        Ok(CombinedContext {
            state,
            summary: scope.summary(encoded.len()),
            hash: blake3::hash(&encoded).to_hex().to_string(),
        })
    }
}
