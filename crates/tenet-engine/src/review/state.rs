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
        has_complete_identity: bool,
    ) -> ContractState {
        if let Some(record) = self
            .list_records()
            .rev()
            .find(|record| record.contract == contract.name && record.input == input)
            && (has_complete_identity || record.conclusion == ContractStatus::Unresolved)
        {
            return ContractState::Settled {
                conclusion: record.conclusion,
                record: Some(record.id.clone()),
            };
        }
        if !has_complete_identity {
            return ContractState::Pending {
                reason: Pending::NotAssessed,
            };
        }
        let is_changed = plan.mode == Mode::Diff && plan.contract_changes.contains(&contract.path);
        let reason = match model {
            Some(
                ContractStatus::Verified
                | ContractStatus::Preserved
                | ContractStatus::Unaffected
                | ContractStatus::Clear,
            ) if is_changed => Pending::ContractChanged,
            Some(
                conclusion @ (ContractStatus::Verified
                | ContractStatus::Preserved
                | ContractStatus::Unaffected
                | ContractStatus::Clear),
            ) => {
                return ContractState::Settled {
                    conclusion,
                    record: None,
                };
            },
            Some(ContractStatus::Failed) => Pending::ModelFailed,
            Some(ContractStatus::Unresolved) => Pending::ModelUnresolved,
            None => Pending::NotAssessed,
        };
        ContractState::Pending { reason }
    }
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    reason = "Source-identity settlement is tested beside its owner."
)]
mod tests {
    use std::fs;

    use anyhow::Result;

    use crate::{
        ContractState,
        ContractStatus,
        Pending,
        Review,
        Selection,
        plan,
        review::{Entry, Record},
    };

    #[test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "A test reports its failure by panicking."
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "One scenario checks every existing conclusion against unavailable source \
                  identity."
    )]
    fn unidentifiable_source_never_keeps_substantive_records_or_model_claims_current() -> Result<()>
    {
        let root = tempfile::tempdir()?;
        let document = root.path().join(".contracts/001-requirement");
        fs::create_dir_all(&document)?;
        fs::write(
            document.join("CONTRACT.md"),
            "---\nname: requirement\nmessage: Preserve failures.\n---\n## Rules\nPreserve \
             failures.\n",
        )?;
        let selected = plan(root.path(), &Selection::default())?;
        let contract = selected
            .contracts
            .first()
            .ok_or_else(|| anyhow::anyhow!("missing contract"))?;
        for conclusion in [
            ContractStatus::Verified,
            ContractStatus::Failed,
            ContractStatus::Clear,
            ContractStatus::Preserved,
            ContractStatus::Unaffected,
            ContractStatus::Unresolved,
        ] {
            let mut review = Review::new_ephemeral();
            review.entries.push(Entry::Record(Record {
                id: "legacy".into(),
                at: 0,
                rev: 1,
                contract: contract.name.clone(),
                input: "same-error-string".into(),
                conclusion,
                evidence: Vec::new(),
                message: "Legacy record".into(),
                model: None,
            }));
            let state = review.compute_state(
                &selected,
                contract,
                "same-error-string",
                Some(conclusion),
                false,
            );
            assert_eq!(state.is_pending(), conclusion != ContractStatus::Unresolved);
            let without_record =
                review.compute_state(&selected, contract, "other-input", Some(conclusion), false);
            assert!(matches!(
                without_record,
                ContractState::Pending {
                    reason: Pending::NotAssessed
                }
            ));
        }
        Ok(())
    }
}
