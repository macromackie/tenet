use core::ops::Range;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Serialize)]
pub(super) struct ContractEntry<'plan, Selected, Omitted> {
    pub name: &'plan str,
    pub path: &'plan Path,
    pub scope: &'plan Path,
    pub document: &'plan str,
    pub hash: &'plan str,
    pub rules_span: &'plan Range<usize>,
    pub requested_scope: crate::RequestedScope,
    pub selected_files: Selected,
    pub omitted_files: Omitted,
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Serialize)]
pub(super) struct Limits {
    pub source_bytes: usize,
    pub capture_bytes: usize,
    pub packet_bytes: usize,
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Serialize)]
pub(super) struct Side<'plan> {
    text: &'plan str,
    hash: String,
}

impl<'plan> Side<'plan> {
    pub(super) fn new(text: &'plan str) -> Self {
        Self {
            text,
            hash: super::super::hash_content(text),
        }
    }
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the serialized key order."
)]
#[derive(Serialize)]
pub(super) struct SourceEntry<'plan> {
    pub kind: crate::ChangeKind,
    pub previous_path: &'plan Option<PathBuf>,
    pub before: Option<Side<'plan>>,
    pub after: Option<Side<'plan>>,
}
