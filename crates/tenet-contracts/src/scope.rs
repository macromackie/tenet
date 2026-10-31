//! Which files a contract governs: those beneath its scope directory, narrowed by optional `paths`
//! inclusions and `!` exclusions.

use std::path::Path;

use globset::{GlobBuilder, GlobSet};

use crate::document::DocumentProblem;

pub(crate) const CONTRACTS: &str = ".contracts";
/// The path segments that name the current or parent directory.
const DOT_SEGMENTS: [&str; 2] = [".", ".."];

/// Compiled `paths` patterns.
///
/// A file matches when an inclusion matches it and no `!` exclusion does; without inclusions,
/// every file matches unless excluded. Files under `.contracts` match only inclusions that name
/// `.contracts`, so a broad pattern such as `**/*.md` never turns contract documents into
/// subjects.
#[derive(Clone, Debug)]
pub(crate) struct Patterns {
    contracts: GlobSet,
    excluded: GlobSet,
    files: GlobSet,
}

impl Patterns {
    pub(crate) fn is_match(&self, relative: &Path, contract_file: bool) -> bool {
        let included = if contract_file {
            self.contracts.is_match(relative)
        } else {
            self.files.is_match(relative)
        };
        included && !self.excluded.is_match(relative)
    }

    pub(crate) fn new(patterns: &[String]) -> Result<Self, DocumentProblem> {
        if patterns.is_empty() {
            return Err(DocumentProblem::NoPaths);
        }
        let mut files = Vec::new();
        let mut contracts = Vec::new();
        let mut excluded = Vec::new();
        for pattern in patterns {
            let (glob, exclusion) = match pattern.strip_prefix('!') {
                Some(glob) => (glob, true),
                None => (pattern.as_str(), false),
            };
            if glob.is_empty()
                || glob.starts_with('/')
                || glob.split('/').any(|part| DOT_SEGMENTS.contains(&part))
            {
                return Err(DocumentProblem::PathOutside {
                    pattern: pattern.clone(),
                });
            }
            let compiled = GlobBuilder::new(glob)
                .literal_separator(true)
                .build()
                .map_err(|source| {
                    DocumentProblem::PathGlob {
                        pattern: pattern.clone(),
                        source,
                    }
                })?;
            if exclusion {
                excluded.push(compiled);
                continue;
            }
            if glob.split('/').any(|part| part == CONTRACTS) {
                contracts.push(compiled.clone());
            }
            files.push(compiled);
        }
        if files.is_empty() {
            files.push(GlobBuilder::new("**").build()?);
        }
        Ok(Self {
            contracts: GlobSet::new(&contracts)?,
            excluded: GlobSet::new(&excluded)?,
            files: GlobSet::new(&files)?,
        })
    }
}
