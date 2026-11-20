//! Read the evidence for project-owned commands without implicitly executing repository code.

use std::path::Path;

use tenet_checks::{
    behavior::{Finding, Outcome},
    mechanical::{self, Evidence, Freshness},
};
use tenet_engine::Plan;

use crate::{error::CommandError, report_format::quote_argument};

pub(super) fn inspect(plan: &Plan, state: &Path) -> Result<Vec<Finding>, CommandError> {
    let evidence = mechanical::inspect_selected_local(plan, state, None)?;
    Ok(evidence.into_iter().map(build_finding).collect())
}

fn build_finding(row: Evidence) -> Finding {
    let name = row.name.as_str();
    let outcome = match (row.freshness, row.observed_outcome) {
        (Freshness::Current, Some(mechanical::Outcome::Pass)) => Outcome::Passed,
        (Freshness::Current, Some(mechanical::Outcome::Fail)) => Outcome::Failed,
        _ => Outcome::Unresolved,
    };
    let message = if row.freshness == Freshness::Current {
        row.message
            .or(row.reason)
            .unwrap_or_else(|| "No observation was recorded.".into())
    } else {
        let selection = quote_argument(&format!("{}/{name}", row.contract));
        format!(
            "Run the trusted project check with tenet check --local {selection} on the current \
             code."
        )
    };
    Finding {
        check: format!("project-{name}"),
        contract: row.contract,
        cost_usd: None,
        explanation_cost_usd: None,
        explanation_requests: None,
        generation_requests: None,
        message,
        outcome,
        pin_reused: None,
        requested: false,
        reused: true,
        subject: None,
    }
}
