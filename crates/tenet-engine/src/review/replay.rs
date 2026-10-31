//! Validate appended log entries before adding them to in-memory review history.

use std::{collections::HashMap, path::Path};

use super::{Entry, Review, ReviewError, Revision, log, record, store};

impl Review {
    pub(super) fn append(&self, entry: Entry) -> Result<(), ReviewError> {
        let store = self.store.as_ref().ok_or(ReviewError::Ephemeral)?;
        Ok(store::append(
            &store.resolve_path(store::EVENTS),
            log::FORMAT,
            &[entry],
        )?)
    }

    /// Refuse an entry whose references to earlier entries `check` or `record` could not have
    /// written. A record in the current format must also pass every rule `record` applies; one
    /// Tenet 0.8 wrote keeps the rules of its time, so only its references are checked.
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    fn check(&self, entry: &Entry, format: u64) -> Result<(), ReviewError> {
        match entry {
            Entry::Revision(revision) => {
                if self.revisions.contains_key(&revision.rev) {
                    return Err(ReviewError::DuplicateRevision(revision.rev));
                }
            },
            Entry::Assessment(assessment) => {
                if let Some(carried) = assessment.carried_from {
                    let _: &Revision = self.find_revision(carried)?;
                }
            },
            Entry::Record(record) => {
                self.find_revision(record.rev)
                    .and_then(|revision| {
                        let assessment = (self.list_assessments_in(record.rev))
                            .find(|assessment| {
                                assessment.contract == record.contract
                                    && assessment.input == record.input
                            })
                            .ok_or_else(|| {
                                ReviewError::OtherInput {
                                    rev: record.rev,
                                    contract: record.contract.clone(),
                                }
                            })?;
                        if format == log::FORMAT {
                            record::validate(record, assessment, revision, self)?;
                        }
                        Ok(())
                    })
                    .map_err(|error| {
                        ReviewError::InvalidRecord {
                            id: record.id.clone(),
                            contract: record.contract.clone(),
                            reason: Box::new(error),
                        }
                    })?;
            },
        }
        Ok(())
    }

    /// Read lines that follow those already read, refusing any that `check` or `record` would not
    /// have written, so an edited line cannot settle a contract.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The line count is bounded by the store file's size."
    )]
    pub(super) fn extend(&mut self, path: &Path, text: &str, end: u64) -> Result<(), ReviewError> {
        let mut moved_files = HashMap::new();
        for line in store::read_lines(path, text, self.lines) {
            let (number, format, body) = line?;
            let entry = log::parse(format, body, self.find_last_revision(), &mut moved_files)
                .map_err(ReviewError::from)
                .and_then(|entry| self.check(&entry, format).map(|()| entry))
                .map_err(|error| {
                    ReviewError::Line {
                        path: path.to_owned(),
                        number,
                        source: Box::new(error),
                    }
                })?;
            if let Entry::Revision(revision) = &entry {
                let _: Option<usize> = self.revisions.insert(revision.rev, self.entries.len());
            }
            self.entries.push(entry);
        }
        self.lines += text.lines().count();
        self.end = end;
        Ok(())
    }

    /// The revision a following assessment belongs to: the latest, unless a record came after it.
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "The entries are nonempty when `last` returns one, and `revisions` holds \
                  positions of existing entries."
    )]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The remaining variants take no action here, and listing each would repeat the \
                  enum."
    )]
    fn find_last_revision(&mut self) -> Option<&mut Revision> {
        let position = match self.entries.last()? {
            Entry::Revision(_) => self.entries.len() - 1,
            Entry::Assessment(assessment) => self.revisions[&assessment.rev],
            Entry::Record(_) => return None,
        };
        match &mut self.entries[position] {
            Entry::Revision(revision) => Some(revision),
            _ => None,
        }
    }
}
