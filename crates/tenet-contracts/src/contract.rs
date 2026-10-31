use core::ops::Range;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::scope::{CONTRACTS, Patterns};

/// A parsed contract: its name, scope, rules, and source document.
#[derive(Clone, Debug, Serialize)]
#[expect(
    clippy::partial_pub_fields,
    reason = "the compiled `paths` patterns use globset types that stay out of the public API"
)]
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "The document parser initializes compiled paths inside this crate; the field stays \
              out of the public API."
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
    /// [`Contract::is_governing`].
    #[serde(skip)]
    pub(crate) paths: Option<Patterns>,
    pub rules: String,
    pub rules_line: usize,
    pub rules_span: Range<usize>,
    pub scope: PathBuf,
}

impl Contract {
    /// Whether this contract governs `path`, given relative to the project root. Without `paths`,
    /// it governs every file beneath its scope except those under `.contracts`.
    #[must_use]
    #[inline]
    pub fn is_governing(&self, path: &Path) -> bool {
        let Ok(relative) = path.strip_prefix(&self.scope) else {
            return false;
        };
        let contract_file = relative
            .components()
            .any(|part| part == Component::Normal(CONTRACTS.as_ref()));
        match &self.paths {
            None => !contract_file,
            Some(patterns) => patterns.is_match(relative, contract_file),
        }
    }
}
