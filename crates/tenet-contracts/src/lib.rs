//! Numbered Markdown requirements.

mod lock;
mod markdown;
mod parse;

use std::{ops::Range, path::PathBuf};

use serde::Serialize;

pub use lock::{DEFAULT_SOURCE, DEFAULT_SOURCE_URL, Entry, LOCK_FILE, Lock, content_hash};
pub use parse::{DocumentError, is_kebab, name_of, parse, with_name};

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
}
