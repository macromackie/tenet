use std::collections::BTreeSet;

use super::conclusion::Completion;
use crate::Mode;
use crate::change::check_size;
use anyhow::Result;

impl Completion<'_> {
    pub(super) fn evidence(&self) -> Result<serde_json::Value> {
        let plan = self.evidence.plan;
        let inventory: Vec<_> = plan
            .inventory
            .iter()
            .filter(|path| path.starts_with(&self.contract.scope))
            .collect();
        let selected_files: Vec<_> = plan
            .files
            .iter()
            .filter(|path| plan.in_scope(path, &self.contract.scope))
            .collect();
        let changes = selected_files
            .iter()
            .map(|path| self.evidence.change(path))
            .collect::<Result<Vec<_>>>()?;
        let mut included = BTreeSet::new();
        let mut files = Vec::new();
        for change in &changes {
            included.insert(change.path.clone());
            files.push(change.current_input());
        }
        let mut support_files = Vec::new();
        for change in &plan.context {
            if included.insert(change.path.clone()) {
                files.push(change.current_input());
                support_files.push(&change.path);
            }
        }
        let omitted_source: Vec<_> = inventory
            .iter()
            .filter(|path| !included.contains(**path))
            .collect();
        let changes = if plan.mode == Mode::Diff {
            changes
        } else {
            Vec::new()
        };
        let state = serde_json::json!({
            "contract": self.contract.body,
            "requested_scope": plan.requested_scope(self.contract),
            "selected_files": selected_files,
            "support_files": support_files,
            "inventory": inventory,
            "files": files,
            "changes": changes,
            "omitted_source": omitted_source,
        });
        check_size("combined context", serde_json::to_vec(&state)?.len())?;
        Ok(state)
    }
}
