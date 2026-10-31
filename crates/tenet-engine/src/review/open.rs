//! Open persisted or ephemeral review state and manage its disposable answer cache.

use std::{collections::HashMap, path::Path};

use super::{Answers, Pruned, Review, ReviewError, Store, StoreError, store};
use crate::evidence;

impl Review {
    pub(super) fn keep(&self, text: &str) -> Result<String, StoreError> {
        match &self.store {
            Some(store) => store.write_blob(text),
            None => Ok(evidence::hash_content(text)),
        }
    }

    /// State for one run that keeps nothing, such as a snapshot check.
    #[must_use]
    #[inline]
    pub fn new_ephemeral() -> Self {
        Self {
            answers: Answers::new(None),
            end: 0,
            entries: Vec::new(),
            lines: 0,
            revisions: HashMap::new(),
            store: None,
        }
    }

    /// Open the review state in `directory`, creating it when needed.
    #[inline]
    pub fn open(directory: &Path) -> Result<Self, ReviewError> {
        Self::read_store(Store::create(directory)?)
    }

    /// Open existing review state without writing anything.
    #[inline]
    pub fn open_existing(directory: &Path) -> Result<Self, ReviewError> {
        let store = Store::open_existing(directory)?
            .ok_or_else(|| ReviewError::NoState(directory.to_owned()))?;
        Self::read_store(store)
    }

    /// Remove cached answers no check by this Tenet can reuse. The log and blobs are unchanged.
    #[inline]
    pub fn prune_answers(&self) -> Result<Pruned, StoreError> {
        self.answers.prune()
    }

    /// Open review state without writing anything; a project without any has an empty review.
    #[inline]
    pub fn read(directory: &Path) -> Result<Self, ReviewError> {
        match Store::open_existing(directory)? {
            Some(store) => Self::read_store(store),
            None => Ok(Self::new_ephemeral()),
        }
    }

    /// Read the captured file stored under `hash`, if the store holds it.
    #[inline]
    pub fn read_blob(&self, hash: &str) -> Result<Option<String>, ReviewError> {
        Ok(self
            .store
            .as_ref()
            .ok_or(ReviewError::NoBlobs)?
            .read_blob(hash)?)
    }

    fn read_store(store: Store) -> Result<Self, ReviewError> {
        let events = store.resolve_path(store::EVENTS);
        let mut review = Self {
            answers: Answers::new(Some(store.resolve_path(store::ANSWERS))),
            store: Some(store),
            ..Self::new_ephemeral()
        };
        let (text, end) = store::read(&events, 0)?;
        review.extend(&events, &text, end)?;
        Ok(review)
    }

    /// Answer from earlier identical requests to `provider` that it answered as `model`, unless
    /// `reuse` is off; new answers are always kept.
    #[inline]
    pub fn use_model(
        &mut self,
        provider: &str,
        model: &str,
        reuse: bool,
    ) -> Result<(), StoreError> {
        self.answers.read_reusable(provider, model, reuse)
    }
}
