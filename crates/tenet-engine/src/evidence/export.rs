use std::io::{self, Write};

use anyhow::{Result, ensure};
use serde::{
    Serialize, Serializer,
    ser::{Error, SerializeMap, SerializeSeq, SerializeStruct},
};

use super::{EvidencePacket, MAX_CAPTURE_BYTES};
use crate::{Mode, Plan};

impl Plan {
    /// Export captured evidence without assessing it or reading additional dependencies.
    pub async fn export_evidence(&self, max_bytes: usize) -> Result<Vec<u8>> {
        ensure!(
            (1..=64 * 1024 * 1024).contains(&max_bytes),
            "evidence limit must be between 1 and 67108864 bytes"
        );
        let evidence = self
            .capture_with_limits(max_bytes, MAX_CAPTURE_BYTES)
            .await?;
        let mut output = LimitedOutput {
            bytes: Vec::new(),
            limit: max_bytes,
        };
        serde_json::to_writer(
            &mut output,
            &Export {
                evidence: &evidence,
                max_bytes,
            },
        )?;
        output.write_all(b"\n")?;
        Ok(output.bytes)
    }
}

struct LimitedOutput {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for LimitedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other(format!(
                "encoded evidence exceeds {} bytes; input was not truncated",
                self.limit
            )));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Export<'a> {
    evidence: &'a EvidencePacket<'a>,
    max_bytes: usize,
}

impl Serialize for Export<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let plan = self.evidence.plan;
        let mut state = serializer.serialize_struct("Evidence", 16)?;
        state.serialize_field("version", &1)?;
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
        state.serialize_field("inventory", &plan.inventory)?;
        state.serialize_field(
            "omitted_files",
            &Sequence(
                plan.inventory
                    .iter()
                    .filter(|path| !self.evidence.changes.contains_key(*path)),
            ),
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

#[derive(Serialize)]
struct Limits {
    source_bytes: usize,
    capture_bytes: usize,
    packet_bytes: usize,
}

struct Contracts<'a>(&'a EvidencePacket<'a>);

impl Serialize for Contracts<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let plan = self.0.plan;
        let mut contracts = serializer.serialize_seq(Some(plan.contracts.len()))?;
        for contract in &plan.contracts {
            let selected = Sequence(
                plan.files
                    .iter()
                    .filter(|path| plan.in_scope(path, &contract.scope)),
            );
            let omitted = Sequence(plan.inventory.iter().filter(|path| {
                path.starts_with(&contract.scope) && !self.0.changes.contains_key(*path)
            }));
            #[derive(Serialize)]
            struct Entry<'a, S, O> {
                name: &'a str,
                path: &'a std::path::Path,
                scope: &'a std::path::Path,
                document: &'a str,
                hash: &'a str,
                rules_span: &'a std::ops::Range<usize>,
                requested_scope: crate::RequestedScope,
                selected_files: S,
                omitted_files: O,
            }
            contracts.serialize_element(&Entry {
                name: &contract.name,
                path: &contract.path,
                scope: &contract.scope,
                document: &contract.document,
                hash: &contract.hash,
                rules_span: &contract.rules_span,
                requested_scope: plan.requested_scope(contract),
                selected_files: selected,
                omitted_files: omitted,
            })?;
        }
        contracts.end()
    }
}

struct Sources<'a>(&'a EvidencePacket<'a>);

impl Serialize for Sources<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sources = serializer.serialize_map(Some(self.0.changes.len()))?;
        for (path, change) in &self.0.changes {
            let change = change.as_ref().map_err(|error| {
                S::Error::custom(format!("cannot export {}: {error}", path.display()))
            })?;
            if change.before.is_none() && change.after.is_none() {
                return Err(S::Error::custom(format!(
                    "captured source does not exist: {}",
                    path.display()
                )));
            }
            #[derive(Serialize)]
            struct Entry<'a> {
                kind: crate::ChangeKind,
                previous_path: &'a Option<std::path::PathBuf>,
                before: Option<Side<'a>>,
                after: Option<Side<'a>>,
            }
            sources.serialize_entry(
                path,
                &Entry {
                    kind: change.kind,
                    previous_path: &change.previous_path,
                    before: change.before.as_deref().map(Side::new),
                    after: change.after.as_deref().map(Side::new),
                },
            )?;
        }
        sources.end()
    }
}

#[derive(Serialize)]
struct Side<'a> {
    text: &'a str,
    hash: String,
}

impl<'a> Side<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            hash: blake3::hash(text.as_bytes()).to_hex().to_string(),
        }
    }
}

struct Sequence<I>(I);

impl<I: Iterator + Clone> Serialize for Sequence<I>
where
    I::Item: Serialize,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(None)?;
        for item in self.0.clone() {
            sequence.serialize_element(&item)?;
        }
        sequence.end()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{LimitedOutput, Sequence};

    #[test]
    fn serialized_metadata_stops_at_the_output_bound() {
        let visited = Cell::new(0);
        let paths = (0..1_000_000).inspect(|_| visited.set(visited.get() + 1));
        let mut output = LimitedOutput {
            bytes: Vec::new(),
            limit: 32,
        };
        assert!(serde_json::to_writer(&mut output, &Sequence(paths)).is_err());
        assert!(output.bytes.len() <= 32);
        assert!(visited.get() < 20);
    }
}
