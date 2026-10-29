//! Numbered Markdown requirements, installed skill documents, and the adoption lock.

extern crate alloc;

mod lock;
mod markdown;
mod parse;
mod scope;
mod skill;

use core::ops::Range;
use std::path::PathBuf;

pub use lock::{
    DEFAULT_SOURCE,
    DEFAULT_SOURCE_URL,
    Entry,
    LOCK_FILE,
    Lock,
    LockError,
    SkillEntry,
    hash_contract,
    hash_file,
    hash_skill,
    is_contract_asset,
};
pub use parse::{
    DocumentError,
    DocumentProblem,
    FrontmatterError,
    find_conflict_marker,
    is_kebab,
    name_of,
    parse,
    parse_version,
    with_name,
};
use serde::Serialize;
pub use skill::{is_skill_name, validate_skill};

/// A parsed contract: its name, scope, rules, and source document.
#[derive(Clone, Debug, Serialize)]
#[expect(
    clippy::partial_pub_fields,
    reason = "the compiled `paths` patterns use globset types that stay out of the public API"
)]
pub struct Contract {
    pub body: String,
    #[serde(skip)]
    pub document: String,
    pub hash: String,
    pub message: String,
    pub name: String,
    pub number: u32,
    pub path: PathBuf,
    /// The front matter's `paths` patterns; every file in scope when absent. See
    /// [`Contract::governs`].
    #[serde(skip)]
    paths: Option<scope::Patterns>,
    pub rules: String,
    pub rules_line: usize,
    pub rules_span: Range<usize>,
    pub scope: PathBuf,
}
