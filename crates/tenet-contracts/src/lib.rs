//! Numbered Markdown requirements.

mod markdown;
mod parse;

use std::{ops::Range, path::PathBuf};

use serde::Serialize;

pub use parse::{DocumentError, parse};

#[derive(Clone, Debug, Serialize)]
pub struct Contract {
    pub name: String,
    pub message: String,
    pub number: u32,
    pub path: PathBuf,
    pub scope: PathBuf,
    pub rules: String,
    pub body: String,
    pub applies_to: Option<String>,
    pub rules_line: usize,
    pub rules_span: Range<usize>,
    pub hash: String,
    #[serde(skip)]
    pub document: String,
}
