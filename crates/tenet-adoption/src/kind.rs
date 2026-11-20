//! The two kinds of installed copy, and the rule each of their files follows when upstream
//! changes it.

use core::str::Utf8Error;
use std::path::Path;

use serde::Serialize;
use tenet_contracts::{
    DocumentError,
    FrontmatterError,
    hash_contract,
    hash_file,
    hash_skill,
    split_adaptations,
};

/// Why a copy's file cannot be hashed as the lock records it.
#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum HashError {
    #[error(transparent)]
    Frontmatter(#[from] FrontmatterError),
    #[error(transparent)]
    NotUtf8(#[from] Utf8Error),
}

/// Which lock table a command manages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Contracts,
    Skills,
}

/// Whether an installed copy is unchanged, adapted, edited, or missing locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalState {
    /// A contract's CONTRACT.md that differs from its release only in its final `## Local
    /// adaptations` section.
    Adapted,
    Edited,
    Missing,
    Unchanged,
}

/// How an installed file takes upstream changes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rule {
    /// Other Markdown in a skill: merged three ways.
    Merged,
    /// The copy's CONTRACT.md or SKILL.md: compared without its `name` value and merged three ways.
    Named,
    /// Every other file, such as a contract's assets: replaced while unedited and never merged.
    ReadOnly,
}

impl Kind {
    pub(crate) fn derive_rule(self, path: &Path) -> Rule {
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

    /// The state of a present file of a copy, `path` relative to its folder, against its locked
    /// hash.
    pub(crate) fn derive_state(
        self,
        path: &Path,
        bytes: &[u8],
        locked: &str,
    ) -> Result<LocalState, HashError> {
        if self.hash(path, bytes)? == locked {
            return Ok(LocalState::Unchanged);
        }
        Ok(match self.find_adaptations(path, bytes, locked)? {
            Some(_) => LocalState::Adapted,
            None => LocalState::Edited,
        })
    }

    /// The final `## Local adaptations` section of a contract's CONTRACT.md, `path` relative to its
    /// folder, when the text above it hashes to `locked`. `None` for every other file.
    pub(crate) fn find_adaptations<'bytes>(
        self,
        path: &Path,
        bytes: &'bytes [u8],
        locked: &str,
    ) -> Result<Option<&'bytes str>, HashError> {
        if (self, self.derive_rule(path)) != (Self::Contracts, Rule::Named) {
            return Ok(None);
        }
        let Some(adaptations) = split_adaptations(str::from_utf8(bytes)?)? else {
            return Ok(None);
        };
        Ok((hash_contract(&adaptations.upstream)? == locked).then_some(adaptations.section))
    }

    /// Hash a file of a copy, `path` relative to its folder, as the lock records it.
    pub(crate) fn hash(self, path: &Path, bytes: &[u8]) -> Result<String, HashError> {
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
    pub(crate) const fn name_document(self) -> &'static str {
        match self {
            Self::Contracts => "CONTRACT.md",
            Self::Skills => "SKILL.md",
        }
    }

    /// Validate a copy's CONTRACT.md or SKILL.md; `path` is relative to the project root.
    pub(crate) fn validate(self, path: &Path, document: &str) -> Result<(), DocumentError> {
        match self {
            Self::Contracts => tenet_contracts::parse(path, document).map(drop),
            Self::Skills => tenet_contracts::validate_skill(path, document),
        }
    }
}
