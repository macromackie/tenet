#[cfg(test)]
mod tests;

use alloc::collections::BTreeMap;
use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, anyhow};
use serde::{Deserialize, Serialize};
use tenet_checks::behavior::AuditModuleReport;

use crate::error::CommandError;

const QUEUE_FILE: &str = "audit.json";
const QUEUE_VERSION: u64 = 1;

/// Resumable audit queue persisted at `.tenet/audit.json`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(in crate::commands) struct AuditQueue {
    pub deep: bool,
    pub modules: BTreeMap<String, AuditModuleReport>,
    pub version: u64,
}

impl AuditQueue {
    /// Return whether `module_name` already has a saved audit report in this queue.
    pub(super) fn has_module(&self, module_name: &str) -> bool {
        self.modules.contains_key(module_name)
    }

    /// Create an empty queue for the requested audit depth.
    pub(super) const fn new(deep: bool) -> Self {
        Self {
            deep,
            modules: BTreeMap::new(),
            version: QUEUE_VERSION,
        }
    }

    /// Read an existing resumable audit queue from `state`, or start fresh when absent or
    /// requested.
    pub(super) fn read(state: &Path, deep: bool, fresh: bool) -> Result<Self, CommandError> {
        if fresh {
            return Ok(Self::new(deep));
        }
        let path = read_queue_path(state);
        if !path.is_file() {
            return Ok(Self::new(deep));
        }
        let bytes =
            fs::read(&path).with_context(|| format!("read audit state {:?}", path.display()))?;
        let Ok(saved) = serde_json::from_slice::<Self>(&bytes) else {
            return Ok(Self::new(deep));
        };
        if saved.version != QUEUE_VERSION || (deep && !saved.deep) {
            return Ok(Self::new(deep));
        }
        Ok(saved)
    }

    /// Load an existing `.tenet/audit.json` strictly without creating or modifying it.
    pub(in crate::commands) fn read_existing(state: &Path) -> Result<Self, CommandError> {
        let path = read_queue_path(state);
        if !path.is_file() {
            return Err(anyhow!(
                "no saved audit state found at {:?}; run `tenet audit` first",
                path.display()
            )
            .into());
        }
        let bytes =
            fs::read(&path).with_context(|| format!("read audit state {:?}", path.display()))?;
        let saved: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse audit state {:?}", path.display()))?;
        if saved.version != QUEUE_VERSION {
            return Err(anyhow!(
                "unsupported audit state version {:?} in {:?}",
                saved.version,
                path.display()
            )
            .into());
        }
        Ok(saved)
    }

    /// Persist the audit queue atomically inside `state`.
    pub(super) fn save(&self, state: &Path) -> Result<(), CommandError> {
        fs::create_dir_all(state)
            .with_context(|| format!("create state directory {:?}", state.display()))?;
        let target = read_queue_path(state);
        let temporary = state.join(".audit.json.tmp");
        let serialized = serde_json::to_vec_pretty(self).context("encode audit queue")?;
        fs::write(&temporary, serialized)
            .with_context(|| format!("write temporary audit state {:?}", temporary.display()))?;
        fs::rename(&temporary, &target)
            .with_context(|| format!("save audit state {:?}", target.display()))?;
        Ok(())
    }

    /// Insert or replace one module's audit report.
    pub(super) fn upsert(&mut self, report: AuditModuleReport) {
        let _prior = self.modules.insert(report.name.clone(), report);
    }
}

fn read_queue_path(state: &Path) -> PathBuf {
    state.join(QUEUE_FILE)
}
