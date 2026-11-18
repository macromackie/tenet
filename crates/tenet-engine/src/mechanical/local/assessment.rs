use std::io;

use tenet_contracts::Contract;

use super::{Binding, CheckName, Evidence, Freshness, Record, Snapshot};

pub(super) fn report(
    contract: &Contract,
    name: &str,
    previous: Option<&Record>,
    current: &io::Result<(Binding, Snapshot)>,
) -> Evidence {
    let mut row = Evidence {
        checker: current
            .as_ref()
            .ok()
            .map(|(_, snap)| snap.program.display().to_string()),
        contract: contract.name.clone(),
        finding: previous.and_then(|record| record.observation.finding.clone()),
        freshness: Freshness::Missing,
        message: previous.map(|saved| saved.observation.message.clone()),
        name: CheckName::Local(name.to_owned()),
        observed_outcome: previous.map(|saved| saved.observation.outcome),
        reason: None,
        replay: Some(format!(
            "tenet check --local {}/{name} --fresh",
            contract.name
        )),
        reused: None,
    };
    match (current, previous) {
        (Err(error), _) => {
            row.freshness = Freshness::Unresolved;
            row.reason = Some(error.to_string());
        },
        (Ok((_, latest)), Some(saved)) => {
            if let Some(snapshot) = &saved.snapshot {
                row.reason = snapshot.find_difference(latest);
                row.freshness = if row.reason.is_some() {
                    Freshness::Stale
                } else {
                    Freshness::Current
                };
            } else {
                row.freshness = Freshness::Unresolved;
            }
        },
        (Ok(_), None) => {},
    }
    row
}

pub(super) fn recheck_inputs(
    row: &mut Evidence,
    before: &Snapshot,
    after: io::Result<(Binding, Snapshot)>,
) {
    match after {
        Ok((_, latest)) => {
            if let Some(reason) = before.find_difference(&latest) {
                row.freshness = Freshness::Stale;
                row.reason = Some(reason);
            }
        },
        Err(error) => {
            row.freshness = Freshness::Unresolved;
            row.reason = Some(error.to_string());
        },
    }
}
