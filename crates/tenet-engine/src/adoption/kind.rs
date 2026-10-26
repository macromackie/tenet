//! The two kinds of installed copy, and the rule each of their files follows when upstream
//! changes it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tenet_contracts::{contract_hash, file_hash, skill_hash};

use super::relative;

/// Which lock table a command manages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Contracts,
    Skills,
}

/// How an installed file takes upstream changes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Rule {
    /// The copy's CONTRACT.md or SKILL.md: compared without its `name` value and merged three ways.
    Named,
    /// Other Markdown in a skill: merged three ways.
    Merged,
    /// Every other file, such as a contract's assets: replaced while unedited and never merged.
    ReadOnly,
}

impl Kind {
    /// The library folder, lock table, and command for this kind.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Contracts => "contracts",
            Kind::Skills => "skills",
        }
    }

    /// The document that names an installed copy.
    pub(super) fn document(self) -> &'static str {
        match self {
            Kind::Contracts => "CONTRACT.md",
            Kind::Skills => "SKILL.md",
        }
    }

    pub(super) fn rule(self, path: &Path) -> Rule {
        if path == Path::new(self.document()) {
            Rule::Named
        } else if self == Kind::Skills
            && path.extension().is_some_and(|extension| extension == "md")
        {
            Rule::Merged
        } else {
            Rule::ReadOnly
        }
    }

    /// The lock key for a command's path: a contract's CONTRACT.md, given it or its folder, or a
    /// skill's folder, given it or its SKILL.md.
    pub(super) fn key(self, path: &Path) -> Result<PathBuf> {
        let path = relative(path)?;
        let named = path.file_name().is_some_and(|name| name == self.document());
        Ok(match (self, named) {
            (Kind::Contracts, true) | (Kind::Skills, false) => path,
            (Kind::Contracts, false) => path.join(self.document()),
            (Kind::Skills, true) => path.parent().context("skill has no folder")?.to_owned(),
        })
    }

    /// Hash a file of a copy, `path` relative to its folder, as the lock records it.
    pub(super) fn hash(self, path: &Path, bytes: &[u8]) -> Result<String> {
        match (self, self.rule(path)) {
            (Kind::Contracts, Rule::Named) => contract_hash(str::from_utf8(bytes)?),
            (Kind::Skills, Rule::Named) => skill_hash(str::from_utf8(bytes)?),
            (_, Rule::Merged | Rule::ReadOnly) => Ok(file_hash(bytes)),
        }
    }

    /// Validate a copy's CONTRACT.md or SKILL.md; `path` is relative to the project root.
    pub(super) fn validate(self, path: &Path, document: &str) -> Result<()> {
        match self {
            Kind::Contracts => tenet_contracts::parse(path, document).map(drop),
            Kind::Skills => tenet_contracts::validate_skill(path, document),
        }
    }
}
