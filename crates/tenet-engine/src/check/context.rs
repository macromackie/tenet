use std::collections::BTreeSet;

use super::conclusion::Completion;
use crate::change::check_size;
use crate::{Mode, Status};
use anyhow::Result;

impl Completion<'_> {
    pub(super) fn evidence(&self) -> Result<serde_json::Value> {
        let inventory: Vec<_> = self
            .plan
            .inventory
            .iter()
            .filter(|path| path.starts_with(&self.contract.scope))
            .collect();
        let selected_files: Vec<_> = self
            .plan
            .files
            .iter()
            .filter(|path| self.plan.in_scope(path, &self.contract.scope))
            .collect();
        let changes = selected_files
            .iter()
            .map(|path| self.plan.change(path))
            .collect::<Result<Vec<_>>>()?;
        let mut included = BTreeSet::new();
        let mut files = Vec::new();
        for change in &changes {
            let excluded = self.results.iter().any(|result| {
                result.path == change.path.to_string_lossy()
                    && result.status == Status::NotApplicable
            });
            if !excluded {
                included.insert(change.path.clone());
                files.push(change.current_input());
            }
        }
        let mut support_files = Vec::new();
        for change in &self.plan.context {
            if included.insert(change.path.clone()) {
                files.push(change.current_input());
                support_files.push(&change.path);
            }
        }
        let omitted_source: Vec<_> = inventory
            .iter()
            .filter(|path| !included.contains(**path))
            .collect();
        let changes = if self.plan.mode == Mode::Diff {
            changes
        } else {
            Vec::new()
        };
        let state = serde_json::json!({
            "contract": self.contract.body,
            "requested_scope": self.plan.requested_scope(self.contract),
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
