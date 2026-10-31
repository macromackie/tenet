use std::{io::Write as _, path::Path};

use anyhow::anyhow;
use tenet_engine::adoption;
use tokio::signal;

use super::prepare;
use crate::{
    cli::{ExitStatus, Export},
    error::CommandError,
    output,
};

pub(super) fn validate(root: &Path) -> Result<ExitStatus, CommandError> {
    let contracts = tenet_engine::discover_contracts(root)?;
    let recorded = adoption::validate_lock(root)?;
    // A project may install skills without having contracts of its own.
    if contracts.is_empty() && recorded.skills == 0 {
        return Err(anyhow!("no contracts found").into());
    }
    if !contracts.is_empty() {
        writeln!(output::open_stdout(), "{} contracts valid", contracts.len())?;
    }
    if recorded.contracts > 0 {
        writeln!(
            output::open_stdout(),
            "{} installed contracts recorded",
            recorded.contracts
        )?;
    }
    if recorded.skills > 0 {
        writeln!(
            output::open_stdout(),
            "{} installed skills recorded",
            recorded.skills
        )?;
    }
    Ok(ExitStatus::Passed)
}

#[expect(
    clippy::as_conversions,
    reason = "The converted request budgets and byte caps fit the target type."
)]
#[expect(
    clippy::integer_division_remainder_used,
    reason = "tokio::select! expands to a remainder for its branch order."
)]
pub(super) async fn export(root: &Path, export: &Export) -> Result<ExitStatus, CommandError> {
    let (plan, _selection, _snapshot) = prepare(root, &export.input)?;
    let capture = plan.export_evidence(export.max_bytes as usize);
    let bytes = tokio::select! {
        result = capture => result?,
        signal = signal::ctrl_c() => {
            signal?;
            return Ok(ExitStatus::Cancelled);
        }
    };
    output::open_stdout().write_all(&bytes)?;
    Ok(ExitStatus::Passed)
}
