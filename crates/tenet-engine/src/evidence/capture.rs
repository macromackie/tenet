mod budget;
#[cfg(test)]
mod fixture;
#[cfg(test)]
mod identity_tests;
#[cfg(test)]
mod limits_tests;

use alloc::{collections::BTreeSet, sync::Arc};

use budget::{CaptureBudget, measure};
use tokio::task::yield_now;

use super::{CaptureError, EvidenceError, EvidencePacket, source::CapturedSource};
use crate::{Excerpt, Plan, change::MAX_CAPTURE_BYTES};

pub(super) fn measure_context(value: &impl serde::Serialize) -> Result<usize, serde_json::Error> {
    measure(value)
}

fn capture_excerpt(
    source: &mut CapturedSource,
    requested: Option<&[[usize; 2]]>,
) -> Option<Excerpt> {
    let lines = requested?;
    let change = source.change.as_ref().ok()?;
    match Excerpt::new(change, lines.to_vec()) {
        Ok(named) => Some(named),
        Err(error) => {
            source.change = Err(Arc::new(CaptureError::Excerpt(error.clone())));
            Some(Excerpt::new_unavailable(lines.to_vec(), error))
        },
    }
}

impl Plan {
    /// Capture each selected and supporting source once, retaining exact identities even when text
    /// cannot fit.
    #[inline]
    pub async fn capture(&self) -> Result<EvidencePacket<'_>, EvidenceError> {
        self.capture_with_limits(MAX_CAPTURE_BYTES, MAX_CAPTURE_BYTES)
            .await
    }

    pub(super) async fn capture_with_limits(
        &self,
        max_source_bytes: usize,
        max_capture_bytes: usize,
    ) -> Result<EvidencePacket<'_>, EvidenceError> {
        let document_bytes = self
            .contracts
            .iter()
            .map(|contract| contract.document.len())
            .sum();
        if document_bytes > max_source_bytes {
            return Err(EvidenceError::ContractsTooLarge(max_source_bytes));
        }
        let mut encoded_bytes = 0_usize;
        for contract in &self.contracts {
            encoded_bytes = encoded_bytes.saturating_add(measure(&(contract, &contract.document))?);
        }
        if encoded_bytes > max_capture_bytes {
            return Err(EvidenceError::CaptureTooLarge(max_capture_bytes));
        }
        let mut budget = CaptureBudget::new(
            document_bytes,
            encoded_bytes,
            max_source_bytes,
            max_capture_bytes,
        );
        let mut visited_paths = BTreeSet::new();
        for path in self.context.iter().chain(&self.files) {
            if !visited_paths.insert(path) {
                continue;
            }
            let mut captured = CapturedSource::read(self, path, max_source_bytes).await;
            let excerpt =
                capture_excerpt(&mut captured, self.excerpts.get(path).map(Vec::as_slice));
            budget.insert(path.clone(), captured, excerpt)?;
            yield_now().await;
        }
        Ok(budget.into_packet(self))
    }
}
