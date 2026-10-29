use alloc::collections::BTreeMap;
use core::ops::Range;
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{
    Serialize,
    Serializer,
    ser::{Error as _, SerializeMap as _, SerializeSeq as _, SerializeStruct as _},
};

use super::{CaptureError, EvidenceError, EvidencePacket, MAX_CAPTURE_BYTES};
use crate::{Mode, Plan, change::SourceError, report::Causes};

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
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other(format!(
                "encoded evidence exceeds {:?} bytes; input was not truncated",
                self.limit
            )));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
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

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Serialize)]
struct Limits {
    source_bytes: usize,
    capture_bytes: usize,
    packet_bytes: usize,
}

struct Contracts<'plan>(&'plan EvidencePacket<'plan>);

impl Serialize for Contracts<'_> {
    fn serialize<Writer>(&self, serializer: Writer) -> Result<Writer::Ok, Writer::Error>
    where
        Writer: Serializer,
    {
        #[expect(
            clippy::arbitrary_source_item_ordering,
            reason = "Field order is the serialized key order."
        )]
        #[derive(Serialize)]
        struct Entry<'plan, Selected, Omitted> {
            name: &'plan str,
            path: &'plan Path,
            scope: &'plan Path,
            document: &'plan str,
            hash: &'plan str,
            rules_span: &'plan Range<usize>,
            requested_scope: crate::RequestedScope,
            selected_files: Selected,
            omitted_files: Omitted,
        }
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
            contracts.serialize_element(&Entry {
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

struct Sources<'plan>(&'plan EvidencePacket<'plan>);

impl Serialize for Sources<'_> {
    #[expect(
        clippy::too_many_lines,
        reason = "The steps run in one order and share local state; splitting them would add \
                  forwarding functions."
    )]
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    fn serialize<Writer>(&self, serializer: Writer) -> Result<Writer::Ok, Writer::Error>
    where
        Writer: Serializer,
    {
        #[expect(
            clippy::arbitrary_source_item_ordering,
            reason = "Field order is the serialized key order."
        )]
        #[derive(Serialize)]
        struct Entry<'plan> {
            kind: crate::ChangeKind,
            previous_path: &'plan Option<PathBuf>,
            before: Option<Side<'plan>>,
            after: Option<Side<'plan>>,
        }
        let mut sources = serializer.serialize_map(Some(self.0.changes.len()))?;
        for (path, change) in &self.0.changes {
            let captured = match change {
                Ok(captured) => captured,
                Err(error) => {
                    match &**error {
                        CaptureError::Source(SourceError::Unreadable(unreadable)) => {
                            sources.serialize_entry(
                                path,
                                &serde_json::json!({ "unreadable": unreadable.to_string() }),
                            )?;
                            continue;
                        },
                        CaptureError::Source(_) | CaptureError::Excerpt(_) => {
                            return Err(Writer::Error::custom(format!(
                                "cannot export {:?}: {:#}",
                                path.display(),
                                Causes(&**error)
                            )));
                        },
                    }
                },
            };
            if captured.before.is_none() && captured.after.is_none() {
                return Err(Writer::Error::custom(format!(
                    "captured source does not exist: {:?}",
                    path.display()
                )));
            }
            sources.serialize_entry(
                path,
                &Entry {
                    kind: captured.kind,
                    previous_path: &captured.previous_path,
                    before: captured.before.as_deref().map(Side::new),
                    after: captured.after.as_deref().map(Side::new),
                },
            )?;
        }
        sources.end()
    }
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Serialize)]
struct Side<'plan> {
    text: &'plan str,
    hash: String,
}

impl<'plan> Side<'plan> {
    fn new(text: &'plan str) -> Self {
        Self {
            text,
            hash: blake3::hash(text.as_bytes()).to_hex().to_string(),
        }
    }
}

struct Sequence<Items>(Items);

impl<Items: Iterator + Clone> Serialize for Sequence<Items>
where
    Items::Item: Serialize,
{
    fn serialize<Writer>(&self, serializer: Writer) -> Result<Writer::Ok, Writer::Error>
    where
        Writer: Serializer,
    {
        let mut sequence = serializer.serialize_seq(None)?;
        for item in self.0.clone() {
            sequence.serialize_element(&item)?;
        }
        sequence.end()
    }
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    reason = "Unit tests stay beside the private functions they test."
)]
mod tests {
    use core::cell::Cell;

    use super::{LimitedOutput, Sequence};

    #[test]
    fn serialized_metadata_stops_at_the_output_bound() {
        let visited = Cell::new(0_u32);
        let paths = (0..1_000_000_u32).inspect(|_| visited.set(visited.get() + 1));
        let mut output = LimitedOutput {
            bytes: Vec::new(),
            limit: 32,
        };
        assert!(serde_json::to_writer(&mut output, &Sequence(paths)).is_err());
        assert!(output.bytes.len() <= 32);
        assert!(visited.get() < 20);
    }
}
