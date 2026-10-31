//! Resolve a contract's state from its exact input and latest recorded conclusion.

use super::Review;
use crate::{ContractState, ContractStatus, Mode, Pending, Plan};

impl Review {
    /// The latest record for this exact input settles the contract. Otherwise a favorable model
    /// conclusion does, unless a diff changed the contract itself: a diff check assumes the base
    /// complied with the contract, which says nothing about a new or edited requirement. Anything
    /// else waits for a record.
    #[inline]
    pub fn compute_state(
        &self,
        plan: &Plan,
        contract: &tenet_contracts::Contract,
        input: &str,
        model: Option<ContractStatus>,
    ) -> ContractState {
        if let Some(record) = self
            .list_records()
            .rev()
            .find(|record| record.contract == contract.name && record.input == input)
        {
            return ContractState::Settled {
                conclusion: record.conclusion,
                record: Some(record.id.clone()),
            };
        }
        let changed = plan.mode == Mode::Diff && plan.contract_changes.contains(&contract.path);
        match model {
            Some(
                ContractStatus::Verified
                | ContractStatus::Preserved
                | ContractStatus::Unaffected
                | ContractStatus::Clear,
            ) if changed => {
                ContractState::Pending {
                    reason: Pending::ContractChanged,
                }
            },
            Some(
                conclusion @ (ContractStatus::Verified
                | ContractStatus::Preserved
                | ContractStatus::Unaffected
                | ContractStatus::Clear),
            ) => {
                ContractState::Settled {
                    conclusion,
                    record: None,
                }
            },
            Some(ContractStatus::Failed) => {
                ContractState::Pending {
                    reason: Pending::ModelFailed,
                }
            },
            Some(ContractStatus::Unresolved) => {
                ContractState::Pending {
                    reason: Pending::ModelUnresolved,
                }
            },
            None => {
                ContractState::Pending {
                    reason: Pending::NotAssessed,
                }
            },
        }
    }
}
