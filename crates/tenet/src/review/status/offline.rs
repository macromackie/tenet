use std::{
    fs,
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use tenet_checks::behavior::{AuditReplayResult, Outcome};

use crate::{
    cli::ExitStatus,
    error::CommandError,
    output,
    report_format::{format_dim, format_display, format_marker},
};

const OFFLINE_RECEIPT_FILE: &str = "offline-check.json";
const OFFLINE_RECEIPT_VERSION: u64 = 1;

/// Persisted summary of the most recent `tenet check --skip-semantic` (aliases `--offline`,
/// `--replay`) run at `<state>/offline-check.json`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct OfflineReceipt {
    pub behavior: Vec<AuditReplayResult>,
    pub local_count: usize,
    pub native_failed: usize,
    pub native_passed: usize,
    pub recorded_at: String,
    pub version: u64,
}

/// Aggregated pass, failure, and unresolved counts for an offline check receipt.
#[derive(Clone, Copy, Debug)]
pub(super) struct OfflineSummary {
    pub failed: usize,
    pub passed: usize,
    pub unresolved: usize,
}

impl OfflineReceipt {
    /// Compute the CLI exit status reflected by this offline check receipt.
    pub(super) fn exit_status(&self) -> ExitStatus {
        let summary = self.summarize();
        if summary.failed > 0 {
            return ExitStatus::Failed;
        }
        if summary.unresolved > 0 {
            return ExitStatus::Unresolved;
        }
        if summary.passed > 0 || self.local_count > 0 {
            return ExitStatus::Passed;
        }
        ExitStatus::Pending
    }

    /// Construct a new offline check receipt stamped with the current Unix epoch timestamp.
    pub(crate) fn new(
        behavior: Vec<AuditReplayResult>,
        local_count: usize,
        native_failed: usize,
        native_passed: usize,
    ) -> Self {
        Self {
            behavior,
            local_count,
            native_failed,
            native_passed,
            recorded_at: format_unix_timestamp(),
            version: OFFLINE_RECEIPT_VERSION,
        }
    }

    /// Read `<state>/offline-check.json` if present and valid for the current schema version.
    pub(crate) fn read(state: &Path) -> Option<Self> {
        let path = state.join(OFFLINE_RECEIPT_FILE);
        let bytes = fs::read(path).ok()?;
        let receipt: Self = serde_json::from_slice(&bytes).ok()?;
        (receipt.version == OFFLINE_RECEIPT_VERSION).then_some(receipt)
    }

    /// Persist the offline check receipt atomically inside `state`, ensuring `<state>/.gitignore`
    /// keeps review state untracked.
    pub(crate) fn save(&self, state: &Path) -> Result<(), CommandError> {
        fs::create_dir_all(state)
            .with_context(|| format!("create state directory {:?}", state.display()))?;
        let gitignore = state.join(".gitignore");
        if !gitignore.exists() {
            fs::write(&gitignore, "*\n")
                .with_context(|| format!("write state gitignore {:?}", gitignore.display()))?;
        }
        let target = state.join(OFFLINE_RECEIPT_FILE);
        let temporary = state.join(".offline-check.json.tmp");
        let serialized = serde_json::to_vec_pretty(self).context("encode offline check receipt")?;
        fs::write(&temporary, serialized).with_context(|| {
            format!(
                "write temporary offline check receipt {:?}",
                temporary.display()
            )
        })?;
        fs::rename(&temporary, &target)
            .with_context(|| format!("save offline check receipt {:?}", target.display()))?;
        Ok(())
    }

    /// Aggregate native and pinned behavioral outcomes into total pass, fail, and unresolved
    /// counts.
    pub(super) fn summarize(&self) -> OfflineSummary {
        let mut passed = self.native_passed;
        let mut failed = self.native_failed;
        let mut unresolved = 0_usize;
        for row in &self.behavior {
            match row.outcome {
                Outcome::Failed => failed = failed.saturating_add(1),
                Outcome::Passed => passed = passed.saturating_add(1),
                Outcome::Unresolved => unresolved = unresolved.saturating_add(1),
            }
        }
        OfflineSummary {
            failed,
            passed,
            unresolved,
        }
    }
}

/// Render the suggested next command after displaying offline check results in `tenet status`.
pub(super) fn write_offline_next_step(
    out: &mut impl Write,
    receipt: &OfflineReceipt,
) -> Result<(), CommandError> {
    let summary = receipt.summarize();
    let command = if summary.failed > 0 {
        "tenet fix"
    } else {
        "tenet check"
    };
    writeln!(out, "\n Next  {command}")?;
    Ok(())
}

/// Render the `Offline checks` summary and non-passing behavior check rows in `tenet status`.
pub(super) fn write_offline_section(
    out: &mut impl Write,
    receipt: &OfflineReceipt,
) -> Result<(), CommandError> {
    let summary = receipt.summarize();
    writeln!(
        out,
        " {}  {}",
        output::format_styled("Offline checks").bold(),
        format_dim("semantic review skipped; run `tenet check` for full review"),
    )?;
    writeln!(
        out,
        "  {} passed \u{b7} {} failed \u{b7} {} unresolved",
        summary.passed, summary.failed, summary.unresolved,
    )?;
    for row in &receipt.behavior {
        if row.outcome == Outcome::Passed {
            continue;
        }
        let marker = if row.outcome == Outcome::Failed {
            format_marker("FAIL")
        } else {
            format_marker("REVIEW")
        };
        let location = row.subject.as_deref().unwrap_or("-");
        writeln!(
            out,
            "  {marker} {}/{}  {}",
            output::format_styled(&row.contract).bold(),
            row.check,
            format_dim(location),
        )?;
        writeln!(out, "    {}", format_display(&row.message))?;
    }
    Ok(())
}

fn format_unix_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_u64, |duration| duration.as_secs());
    seconds.to_string()
}
