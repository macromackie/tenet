//! The two kinds of installed copy, and the rule each of their files follows when upstream
//! changes it.

use std::path::{Path, PathBuf};

use tenet_contracts::{DocumentError, hash_contract, hash_file, hash_skill};

use super::{AdoptionError, HashError, make_relative};

/// Which lock table a command manages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Contracts,
    Skills,
}

/// How an installed file takes upstream changes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Rule {
    /// Other Markdown in a skill: merged three ways.
    Merged,
    /// The copy's CONTRACT.md or SKILL.md: compared without its `name` value and merged three ways.
    Named,
    /// Every other file, such as a contract's assets: replaced while unedited and never merged.
    ReadOnly,
}

impl Kind {
    /// The lock key for a command's path: a contract's CONTRACT.md, given it or its folder, or a
    /// skill's folder, given it or its SKILL.md.
    pub(super) fn derive_key(self, path: &Path) -> Result<PathBuf, AdoptionError> {
        let relative_path = make_relative(path)?;
        let named = relative_path
            .file_name()
            .is_some_and(|name| name == self.name_document());
        Ok(match (self, named) {
            (Self::Contracts, true) | (Self::Skills, false) => relative_path,
            (Self::Contracts, false) => relative_path.join(self.name_document()),
            (Self::Skills, true) => {
                relative_path
                    .parent()
                    .ok_or(AdoptionError::NoSkillFolder)?
                    .to_owned()
            },
        })
    }

    pub(super) fn derive_rule(self, path: &Path) -> Rule {
        if path == Path::new(self.name_document()) {
            Rule::Named
        } else if self == Self::Skills
            && path.extension().is_some_and(|extension| extension == "md")
        {
            Rule::Merged
        } else {
            Rule::ReadOnly
        }
    }

    /// Hash a file of a copy, `path` relative to its folder, as the lock records it.
    pub(super) fn hash(self, path: &Path, bytes: &[u8]) -> Result<String, HashError> {
        match (self, self.derive_rule(path)) {
            (Self::Contracts, Rule::Named) => Ok(hash_contract(str::from_utf8(bytes)?)?),
            (Self::Skills, Rule::Named) => Ok(hash_skill(str::from_utf8(bytes)?)?),
            (_, Rule::Merged | Rule::ReadOnly) => Ok(hash_file(bytes)),
        }
    }

    /// The library folder, lock table, and command for this kind.
    #[must_use]
    #[inline]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Contracts => "contracts",
            Self::Skills => "skills",
        }
    }

    /// The document that names an installed copy.
    pub(super) const fn name_document(self) -> &'static str {
        match self {
            Self::Contracts => "CONTRACT.md",
            Self::Skills => "SKILL.md",
        }
    }

    /// Validate a copy's CONTRACT.md or SKILL.md; `path` is relative to the project root.
    pub(super) fn validate(self, path: &Path, document: &str) -> Result<(), DocumentError> {
        match self {
            Self::Contracts => tenet_contracts::parse(path, document).map(drop),
            Self::Skills => tenet_contracts::validate_skill(path, document),
        }
    }
}
