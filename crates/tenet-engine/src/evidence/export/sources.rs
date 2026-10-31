use serde::{
    Serialize,
    Serializer,
    ser::{Error as _, SerializeMap as _},
};

use super::{
    super::{CaptureError, EvidencePacket},
    wire::{Side, SourceEntry},
};
use crate::{change::SourceError, report::Causes};

pub(super) struct Sources<'plan>(pub &'plan EvidencePacket<'plan>);

impl Serialize for Sources<'_> {
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    fn serialize<Writer>(&self, serializer: Writer) -> Result<Writer::Ok, Writer::Error>
    where
        Writer: Serializer,
    {
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
                &SourceEntry {
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
