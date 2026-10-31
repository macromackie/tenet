mod output;
mod sequence;
mod sources;
mod wire;

use alloc::collections::BTreeMap;
use std::io::Write as _;

use output::LimitedOutput;
use sequence::Sequence;
use serde::{
    Serialize,
    Serializer,
    ser::{SerializeSeq as _, SerializeStruct as _},
};
use sources::Sources;
use wire::{ContractEntry, Limits};

use super::{EvidenceError, EvidencePacket};
use crate::{Mode, Plan, change::MAX_CAPTURE_BYTES};

impl Plan {
    /// Export captured evidence without assessing it or reading additional dependencies.
    #[inline]
    pub async fn export_evidence(&self, max_bytes: usize) -> Result<Vec<u8>, EvidenceError> {
        if !(1..=64 * 1024 * 1024).contains(&max_bytes) {
            return Err(EvidenceError::InvalidLimit);
        }
        let evidence = self
            .capture_with_limits(max_bytes, MAX_CAPTURE_BYTES)
            .await?;
        let mut output = LimitedOutput::new(max_bytes);
        serde_json::to_writer(
            &mut output,
            &Export {
                evidence: &evidence,
                max_bytes,
            },
        )?;
        output.write_all(b"\n")?;
        Ok(output.into_bytes())
    }
}

/// The schema version of the evidence packet that `export` writes.
const VERSION: u64 = 2;

struct Export<'plan> {
    evidence: &'plan EvidencePacket<'plan>,
    max_bytes: usize,
}

impl Serialize for Export<'_> {
    fn serialize<Writer>(&self, serializer: Writer) -> Result<Writer::Ok, Writer::Error>
    where
        Writer: Serializer,
    {
        let plan = self.evidence.plan;
        let mut state = serializer.serialize_struct("Evidence", 16)?;
        state.serialize_field("version", &VERSION)?;
        state.serialize_field("kind", "tenet_evidence")?;
        state.serialize_field("hash_algorithm", "blake3")?;
        state.serialize_field("mode", &plan.mode)?;
        state.serialize_field("base", &plan.base)?;
        state.serialize_field("head", &plan.head)?;
        state.serialize_field(
            "assumption",
            &(plan.mode == Mode::Diff).then_some("The base satisfies the selected contracts."),
        )?;
        state.serialize_field("selected_paths", &plan.selected_paths)?;
        state.serialize_field("selected_files", &plan.files)?;
        state.serialize_field(
            "support_files",
            &Sequence(plan.context.iter().map(|change| &change.path)),
        )?;
        let excerpts: BTreeMap<_, _> = (plan.excerpts.iter())
            .map(|(path, excerpt)| (path, &excerpt.lines))
            .collect();
        state.serialize_field("excerpts", &excerpts)?;
        state.serialize_field(
            "inventory",
            &Sequence(plan.inventory.iter().filter(|path| {
                plan.contracts
                    .iter()
                    .any(|contract| contract.is_governing(path))
            })),
        )?;
        state.serialize_field("contract_changes", &plan.contract_changes)?;
        state.serialize_field("contracts", &Contracts(self.evidence))?;
        state.serialize_field("sources", &Sources(self.evidence))?;
        state.serialize_field(
            "limits",
            &Limits {
                source_bytes: ev_grep_core::MAX_FILE_BYTES,
                capture_bytes: MAX_CAPTURE_BYTES,
                packet_bytes: self.max_bytes,
            },
        )?;
        state.end()
    }
}

struct Contracts<'plan>(&'plan EvidencePacket<'plan>);

impl Serialize for Contracts<'_> {
    fn serialize<Writer>(&self, serializer: Writer) -> Result<Writer::Ok, Writer::Error>
    where
        Writer: Serializer,
    {
        let plan = self.0.plan;
        let mut contracts = serializer.serialize_seq(Some(plan.contracts.len()))?;
        for contract in &plan.contracts {
            let selected = Sequence(
                plan.files
                    .iter()
                    .filter(|path| plan.is_governing(contract, path)),
            );
            let omitted =
                Sequence(plan.inventory.iter().filter(|path| {
                    contract.is_governing(path) && !self.0.changes.contains_key(*path)
                }));
            contracts.serialize_element(&ContractEntry {
                name: &contract.name,
                path: &contract.path,
                scope: &contract.scope,
                document: &contract.document,
                hash: &contract.hash,
                rules_span: &contract.rules_span,
                requested_scope: plan.find_requested_scope(contract),
                selected_files: selected,
                omitted_files: omitted,
            })?;
        }
        contracts.end()
    }
}
