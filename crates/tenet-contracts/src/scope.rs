//! Which files a contract governs: those beneath its scope directory, narrowed by optional `paths`
//! inclusions and `!` exclusions.

use std::path::{Component, Path};

use anyhow::{Result, bail, ensure};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::Contract;

const CONTRACTS: &str = ".contracts";

/// Compiled `paths` patterns. A file matches when an inclusion matches it and no `!` exclusion
/// does; without inclusions, every file matches unless excluded. Files under `.contracts` match
/// only inclusions that name `.contracts`, so a broad pattern such as `**/*.md` never turns
/// contract documents into subjects.
#[derive(Clone, Debug)]
pub(crate) struct Patterns {
    files: GlobSet,
    contracts: GlobSet,
    excluded: GlobSet,
}

impl Patterns {
    pub(crate) fn new(patterns: &[String]) -> Result<Self> {
        ensure!(!patterns.is_empty(), "paths must list at least one pattern");
        let mut files = GlobSetBuilder::new();
        let mut contracts = GlobSetBuilder::new();
        let mut excluded = GlobSetBuilder::new();
        let mut included = false;
        for pattern in patterns {
            let (glob, exclusion) = match pattern.strip_prefix('!') {
                Some(glob) => (glob, true),
                None => (pattern.as_str(), false),
            };
            if glob.is_empty()
                || glob.starts_with('/')
                || glob.split('/').any(|part| part == "." || part == "..")
            {
                bail!(
                    "paths pattern `{pattern}` must be relative to the contract's scope directory"
                );
            }
            let compiled = GlobBuilder::new(glob)
                .literal_separator(true)
                .build()
                .map_err(|error| anyhow::anyhow!("invalid paths pattern `{pattern}`: {error}"))?;
            if exclusion {
                excluded.add(compiled);
                continue;
            }
            included = true;
            if glob.split('/').any(|part| part == CONTRACTS) {
                contracts.add(compiled.clone());
            }
            files.add(compiled);
        }
        if !included {
            files.add(GlobBuilder::new("**").build()?);
        }
        Ok(Self {
            files: files.build()?,
            contracts: contracts.build()?,
            excluded: excluded.build()?,
        })
    }

    fn is_match(&self, relative: &Path, contract_file: bool) -> bool {
        let included = if contract_file {
            self.contracts.is_match(relative)
        } else {
            self.files.is_match(relative)
        };
        included && !self.excluded.is_match(relative)
    }
}

impl Contract {
    /// Whether this contract governs `path`, given relative to the project root. Without `paths`,
    /// it governs every file beneath its scope except those under `.contracts`.
    pub fn governs(&self, path: &Path) -> bool {
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
