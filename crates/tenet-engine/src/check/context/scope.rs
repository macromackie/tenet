use alloc::collections::BTreeSet;
use std::path::PathBuf;

use tenet_contracts::Contract;

use crate::{ContextSummary, Plan, RequestedScope};

pub(crate) struct Scope<'plan> {
    pub inventory: Vec<&'plan PathBuf>,
    pub omitted: Vec<&'plan PathBuf>,
    pub requested: RequestedScope,
    pub selected: Vec<&'plan PathBuf>,
    pub supplied_inventory: Vec<&'plan PathBuf>,
    pub support: Vec<&'plan PathBuf>,
}

impl<'plan> Scope<'plan> {
    pub(crate) fn new(plan: &'plan Plan, contract: &Contract) -> Self {
        let selected: Vec<_> = plan
            .files
            .iter()
            .filter(|path| plan.is_governing(contract, path))
            .collect();
        let mut supplied_paths: BTreeSet<_> = selected.iter().copied().collect();
        let support = plan
            .context
            .iter()
            .filter_map(|change| supplied_paths.insert(&change.path).then_some(&change.path))
            .collect();
        let inventory: Vec<_> = plan
            .inventory
            .iter()
            .filter(|path| contract.is_governing(path))
            .collect();
        let (supplied_inventory, omitted) = inventory
            .iter()
            .copied()
            .partition(|path| supplied_paths.contains(path));
        Self {
            inventory,
            omitted,
            requested: plan.find_requested_scope(contract),
            selected,
            supplied_inventory,
            support,
        }
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The file counts are bounded by the repository's file list."
    )]
    pub(super) fn summarize(&self, plan: &Plan, input_bytes: usize) -> ContextSummary {
        ContextSummary {
            input_bytes,
            limit_bytes: ev_grep_core::MAX_FILE_BYTES,
            included_files: self.selected.len() + self.support.len(),
            selected_files: self.selected.iter().map(|path| (*path).clone()).collect(),
            support_files: self.support.iter().map(|path| (*path).clone()).collect(),
            excerpts: (self.support.iter())
                .filter_map(|path| Some(((*path).clone(), plan.excerpts.get(*path)?.lines.clone())))
                .collect(),
            omitted_files: self.omitted.len(),
            before_omitted: false,
            inventory_counted: false,
            contents_omitted: Vec::new(),
        }
    }
}
