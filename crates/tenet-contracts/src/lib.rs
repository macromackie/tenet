//! Numbered Markdown requirements, installed skill documents, and the adoption lock.

mod lock;
mod markdown;
mod parse;
mod scope;
mod skill;

use std::{ops::Range, path::PathBuf};

use serde::Serialize;

pub use lock::{
    DEFAULT_SOURCE, DEFAULT_SOURCE_URL, Entry, LOCK_FILE, Lock, SkillEntry, contract_hash,
    file_hash, is_contract_asset, skill_hash,
};
pub use parse::{
    DocumentError, conflict_marker, is_kebab, name_of, parse, parse_version, with_name,
};
pub use skill::{is_skill_name, validate_skill};

#[derive(Clone, Debug, Serialize)]
pub struct Contract {
    pub name: String,
    pub message: String,
    pub number: u32,
    pub path: PathBuf,
    pub scope: PathBuf,
    pub rules: String,
    pub body: String,
    pub rules_line: usize,
    pub rules_span: Range<usize>,
    pub hash: String,
    #[serde(skip)]
    pub document: String,
    /// The front matter's `paths` patterns; every file in scope when absent. See [`Contract::governs`].
    #[serde(skip)]
    pub(crate) paths: Option<scope::Patterns>,
}
