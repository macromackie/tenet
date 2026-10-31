//! Query the ordered review log without deriving a second state.

use super::{Assessment, Entry, Record, Review, ReviewError, Revision};

impl Review {
    /// Return every entry of the review log, oldest first.
    #[inline]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The latest assessment of this contract with exactly this input, from any revision.
    #[inline]
    pub fn find_assessment(&self, contract: &str, input: &str) -> Option<&Assessment> {
        self.list_assessments()
            .rev()
            .find(|assessment| assessment.contract == contract && assessment.input == input)
    }

    /// Return revision `rev`, or an error when the log does not hold it.
    #[expect(
        clippy::indexing_slicing,
        reason = "`revisions` holds positions of existing entries."
    )]
    #[inline]
    pub fn find_revision(&self, rev: u64) -> Result<&Revision, ReviewError> {
        match self
            .revisions
            .get(&rev)
            .map(|&position| &self.entries[position])
        {
            Some(Entry::Revision(revision)) => Ok(revision),
            _ => Err(ReviewError::UnknownRevision(rev)),
        }
    }

    /// Return every recorded assessment, oldest first.
    #[inline]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The remaining variants take no action here, and listing each would repeat the \
                  enum."
    )]
    pub fn list_assessments(&self) -> impl DoubleEndedIterator<Item = &Assessment> {
        self.entries.iter().filter_map(|entry| {
            match entry {
                Entry::Assessment(assessment) => Some(assessment),
                _ => None,
            }
        })
    }

    /// The assessments one revision made, which follow it in the log.
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "Positions in `revisions` index existing entries, and the position after one is \
                  at most the entry count."
    )]
    #[inline]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The remaining variants take no action here, and listing each would repeat the \
                  enum."
    )]
    pub fn list_assessments_in(&self, rev: u64) -> impl Iterator<Item = &Assessment> {
        let start = self
            .revisions
            .get(&rev)
            .map_or(self.entries.len(), |position| position + 1);
        self.entries[start..].iter().map_while(|entry| {
            match entry {
                Entry::Assessment(assessment) => Some(assessment),
                _ => None,
            }
        })
    }

    /// Return every record in the log, oldest first.
    #[inline]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The remaining variants take no action here, and listing each would repeat the \
                  enum."
    )]
    pub fn list_records(&self) -> impl DoubleEndedIterator<Item = &Record> {
        self.entries.iter().filter_map(|entry| {
            match entry {
                Entry::Record(record) => Some(record),
                _ => None,
            }
        })
    }

    /// Return every revision in the log, oldest first.
    #[inline]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The remaining variants take no action here, and listing each would repeat the \
                  enum."
    )]
    pub fn list_revisions(&self) -> impl DoubleEndedIterator<Item = &Revision> {
        self.entries.iter().filter_map(|entry| {
            match entry {
                Entry::Revision(revision) => Some(revision),
                _ => None,
            }
        })
    }
}
