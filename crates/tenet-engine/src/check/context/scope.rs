use std::{collections::BTreeSet, path::PathBuf};

use tenet_contracts::Contract;

use crate::{ContextSummary, Plan, RequestedScope};

pub(super) struct Scope<'a> {
    pub requested: RequestedScope,
    pub selected: Vec<&'a PathBuf>,
    pub support: Vec<&'a PathBuf>,
    pub inventory: Vec<&'a PathBuf>,
    pub supplied_inventory: Vec<&'a PathBuf>,
    pub omitted: Vec<&'a PathBuf>,
}

impl<'a> Scope<'a> {
    pub(super) fn new(plan: &'a Plan, contract: &Contract) -> Self {
        let selected: Vec<_> = plan
            .files
            .iter()
            .filter(|path| plan.in_scope(path, &contract.scope))
            .collect();
        let mut supplied: BTreeSet<_> = selected.iter().copied().collect();
        let support = plan
            .context
            .iter()
            .filter_map(|change| supplied.insert(&change.path).then_some(&change.path))
            .collect();
        let inventory: Vec<_> = plan
            .inventory
            .iter()
            .filter(|path| path.starts_with(&contract.scope))
            .collect();
        let (supplied_inventory, omitted) = inventory
            .iter()
            .copied()
            .partition(|path| supplied.contains(path));
        Self {
            requested: plan.requested_scope(contract),
            selected,
            support,
            inventory,
            supplied_inventory,
            omitted,
        }
    }

    pub(super) fn summary(&self, input_bytes: usize) -> ContextSummary {
        ContextSummary {
            input_bytes,
            limit_bytes: ev_grep_core::MAX_FILE_BYTES,
            included_files: self.selected.len() + self.support.len(),
            selected_files: self.selected.iter().map(|path| (*path).clone()).collect(),
            support_files: self.support.iter().map(|path| (*path).clone()).collect(),
            omitted_files: self.omitted.iter().map(|path| (*path).clone()).collect(),
        }
    }
}
