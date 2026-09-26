//! Numbered Markdown requirements and isolated evaluation examples.

mod markdown;
mod parse;

use std::{ops::Range, path::PathBuf};

use serde::{Deserialize, Serialize};

pub use parse::{DocumentError, parse};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    Fail,
    NotApplicable,
    Uncertain,
}

#[derive(Clone, Debug, Serialize)]
pub struct Example {
    pub change: Option<ExampleChange>,
    pub name: String,
    pub path: PathBuf,
    pub expected: Verdict,
    pub source: String,
    pub line: usize,
}

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
    pub examples: Vec<Example>,
    pub rules_line: usize,
    pub rules_span: Range<usize>,
    pub hash: String,
    #[serde(skip)]
    pub document: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExampleChange {
    pub before: Option<String>,
    pub after: Option<String>,
}
