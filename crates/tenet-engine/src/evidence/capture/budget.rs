use alloc::collections::{BTreeMap, BinaryHeap};
use std::{
    io,
    path::{Path, PathBuf},
};

use serde::Serialize;

use super::super::{EvidenceError, EvidencePacket, source::CapturedSource};

#[derive(Eq, Ord, PartialEq, PartialOrd)]
struct Payload {
    encoded_bytes: usize,
    path: PathBuf,
    source_bytes: usize,
}

/// Retain complete payloads within the shared budget, preferring smaller admissible sources.
pub(super) struct CaptureBudget {
    encoded_bytes: usize,
    excerpts: BTreeMap<PathBuf, crate::Excerpt>,
    max_encoded_bytes: usize,
    max_source_bytes: usize,
    payloads: BinaryHeap<Payload>,
    source_bytes: usize,
    sources: BTreeMap<PathBuf, CapturedSource>,
}

struct Counter(usize);

impl io::Write for Counter {
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }
}

pub(super) fn measure(value: &impl Serialize) -> Result<usize, serde_json::Error> {
    let mut output = Counter(0);
    serde_json::to_writer(&mut output, value)?;
    Ok(output.0)
}

fn measure_source(path: &Path, source: &CapturedSource) -> Result<usize, serde_json::Error> {
    match &source.change {
        Ok(change) => {
            measure(&(
                path,
                change,
                &change.after,
                &source.before,
                &source.after,
                source.has_complete_identity,
            ))
        },
        Err(error) => {
            measure(&(
                path,
                error.to_string(),
                &source.before,
                &source.after,
                source.has_complete_identity,
            ))
        },
    }
}

impl CaptureBudget {
    fn drop_largest_payload(&mut self) -> Result<(), EvidenceError> {
        let payload = self
            .payloads
            .pop()
            .ok_or(EvidenceError::CaptureTooLarge(self.max_encoded_bytes))?;
        let captured = self
            .sources
            .get_mut(&payload.path)
            .ok_or(EvidenceError::CaptureTooLarge(self.max_encoded_bytes))?;
        let (bytes, limit) = if self.source_bytes > self.max_source_bytes {
            (self.source_bytes, self.max_source_bytes)
        } else {
            (self.encoded_bytes, self.max_encoded_bytes)
        };
        captured.drop_payload(u64::try_from(bytes).unwrap_or(u64::MAX), limit);
        drop(self.excerpts.remove(&payload.path));
        self.source_bytes = self.source_bytes.saturating_sub(payload.source_bytes);
        self.encoded_bytes = self
            .encoded_bytes
            .saturating_sub(payload.encoded_bytes)
            .saturating_add(measure_source(&payload.path, captured)?);
        Ok(())
    }

    pub(super) fn insert(
        &mut self,
        path: PathBuf,
        source: CapturedSource,
        excerpt: Option<crate::Excerpt>,
    ) -> Result<(), EvidenceError> {
        let excerpt_bytes = match &excerpt {
            Some(named) => {
                measure(&(path.as_path(), &named.lines, named.read_retained_contents()))?
            },
            None => 0,
        };
        let excerpt_payload_bytes = match excerpt
            .as_ref()
            .and_then(crate::Excerpt::read_retained_contents)
        {
            Some(contents) => measure(contents)?,
            None => 0,
        };
        let source_bytes = source
            .read_source_bytes()
            .saturating_add(excerpt_payload_bytes);
        let encoded_bytes = measure_source(&path, &source)?.saturating_add(excerpt_bytes);
        if let Some(named) = excerpt {
            drop(self.excerpts.insert(path.clone(), named));
        }
        self.source_bytes = self.source_bytes.saturating_add(source_bytes);
        self.encoded_bytes = self.encoded_bytes.saturating_add(encoded_bytes);
        if source_bytes > 0 {
            self.payloads.push(Payload {
                encoded_bytes,
                path: path.clone(),
                source_bytes,
            });
        }
        drop(self.sources.insert(path, source));
        while self.source_bytes > self.max_source_bytes
            || self.encoded_bytes > self.max_encoded_bytes
        {
            self.drop_largest_payload()?;
        }
        Ok(())
    }

    pub(super) fn into_packet(self, plan: &crate::Plan) -> EvidencePacket<'_> {
        EvidencePacket {
            changes: self.sources,
            excerpts: self.excerpts,
            plan,
        }
    }

    pub(super) const fn new(
        source_bytes: usize,
        encoded_bytes: usize,
        max_source_bytes: usize,
        max_encoded_bytes: usize,
    ) -> Self {
        Self {
            encoded_bytes,
            excerpts: BTreeMap::new(),
            max_encoded_bytes,
            max_source_bytes,
            payloads: BinaryHeap::new(),
            source_bytes,
            sources: BTreeMap::new(),
        }
    }
}
