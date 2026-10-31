use core::num::ParseIntError;
use std::path::PathBuf;

/// The largest contract document Tenet reads, in bytes.
pub(crate) const DOCUMENT_BYTES: usize = 0x0001_0000;

/// The longest skill name the Agent Skills specification allows.
pub(crate) const SKILL_NAME_CHARACTERS: usize = 64;

/// A contract or skill document Tenet refuses, located at the line to fix.
#[derive(Debug, thiserror::Error)]
#[error("{path:?}:{line:?}: {problem:#}")]
pub struct DocumentError {
    pub line: usize,
    pub path: PathBuf,
    pub problem: DocumentProblem,
}

/// Why Tenet refuses a contract or skill document.
#[derive(Debug, thiserror::Error)]
pub enum DocumentProblem {
    #[error("unresolved merge conflict marker")]
    ConflictMarker,
    #[error("name must be lowercase kebab-case")]
    ContractName,
    #[error("contract directory must be NNN-kebab-case with a positive number")]
    DirectoryFormat,
    #[error("invalid contract directory")]
    DirectoryName,
    #[error("duplicate section: {name:?}")]
    DuplicateSection { name: String },
    #[error(transparent)]
    Frontmatter(#[from] FrontmatterError),
    #[error(transparent)]
    Globs(#[from] globset::Error),
    #[error("expected .contracts/NNN-name/CONTRACT.md")]
    Layout,
    #[error("requires Tenet {required:?}; this is Tenet {:?}", env!("CARGO_PKG_VERSION"))]
    NewerTenet { required: String },
    #[error("contract body must not be empty")]
    NoBody,
    #[error("contract requires .contracts")]
    NoContracts,
    #[error("description must not be empty")]
    NoDescription,
    #[error("contract requires a numbered directory")]
    NoDirectory,
    #[error("message must not be empty")]
    NoMessage,
    #[error("contract directory requires NNN-name")]
    NoNumber,
    #[error("paths must list at least one pattern")]
    NoPaths,
    #[error(transparent)]
    Number(ParseIntError),
    #[error("invalid paths pattern `{pattern:?}`: {source:#}")]
    PathGlob {
        pattern: String,
        source: globset::Error,
    },
    #[error("paths pattern `{pattern:?}` must be relative to the contract's scope directory")]
    PathOutside { pattern: String },
    #[error("requires-tenet must be a version such as 0.11.0, not {required:?}")]
    RequirementFormat { required: String },
    #[error("contract number needs at least three digits")]
    ShortNumber,
    #[error("name {name:?} must equal the skill's folder name {folder:?}")]
    SkillFolder { name: String, folder: String },
    #[error(
        "name {name:?} must use lowercase letters, digits, and single hyphens, at most \
         {SKILL_NAME_CHARACTERS:?} characters"
    )]
    SkillName { name: String },
    #[error("contract exceeds {DOCUMENT_BYTES:?} bytes")]
    TooLarge,
    #[error(transparent)]
    Yaml(#[from] Box<serde_saphyr::Error>),
}

/// Why a document's frontmatter has no readable `name`.
#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum FrontmatterError {
    #[error("missing YAML frontmatter")]
    Missing,
    #[error("frontmatter has no name")]
    NoName,
    #[error("unclosed YAML frontmatter")]
    Unclosed,
}
