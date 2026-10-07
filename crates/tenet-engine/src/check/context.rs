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
            .filter(|p| p.starts_with(&self.contract.scope))
            .collect();
        let changes: Vec<_> = if self.plan.mode == Mode::Diff {
            self.plan
                .files
                .iter()
                .filter(|p| self.plan.in_scope(p, &self.contract.scope))
                .map(|p| self.plan.change(p))
                .collect::<Result<_>>()?
        } else {
            Vec::new()
        };
        let mut files = Vec::new();
        let mut required_bytes = 0;
        let relevant_changes = changes.iter().filter(|change| {
            !self.results.iter().any(|result| {
                result.path == change.path.to_string_lossy()
                    && result.status == Status::NotApplicable
            })
        });
        for change in relevant_changes.chain(&self.plan.context) {
            if let Some(content) = &change.after
                && !files.iter().any(|file: &serde_json::Value| {
                    file["path"] == change.path.to_string_lossy().as_ref()
                })
            {
                required_bytes += content.len();
                files.push(serde_json::json!({"path": change.path, "contents": content}));
            }
        }
        let mut optional = Vec::new();
        for path in &inventory {
            if files
                .iter()
                .any(|file| file["path"] == path.to_string_lossy().as_ref())
            {
                continue;
            }
            let required = self.plan.mode == Mode::Full
                && !self.results.iter().any(|result| {
                    result.path == path.to_string_lossy() && result.status == Status::NotApplicable
                });
            if required {
                let content = crate::change::text(&self.plan.root, path)?;
                required_bytes += content.len();
                check_size("required source", required_bytes)?;
                files.push(serde_json::json!({"path": path, "contents": content}));
            } else {
                optional.push(*path);
            }
        }
        let changes: Vec<_> = changes
            .iter()
            .map(|change| {
                serde_json::json!({
                    "path": change.path,
                    "previous_path": change.previous_path,
                    "kind": change.kind,
                })
            })
            .collect();
        let state = serde_json::json!({
            "contract": self.contract.body,
            "inventory": inventory,
            "files": files,
            "changes": changes,
            "omitted_source": optional,
        });
        check_size(
            "required repository context",
            serde_json::to_vec(&state)?.len(),
        )?;
        Ok(state)
    }
}
