use core::{num::ParseIntError, ops::Range};
use std::path::{Path, PathBuf};

use serde::{Deserialize, de::IgnoredAny};

use crate::{Contract, markdown, scope::Patterns, skill::SKILL_NAME_CHARACTERS};

/// The largest contract document Tenet reads, in bytes.
const DOCUMENT_BYTES: usize = 0x0001_0000;

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
    Yaml(#[from] serde_yaml::Error),
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

/// A contract failure before [`parse`] locates it: on its line, or on line 1.
#[derive(Debug, thiserror::Error)]
enum Refusal {
    #[error(transparent)]
    Located(#[from] DocumentError),
    #[error(transparent)]
    Unlocated(#[from] DocumentProblem),
}

/// This Tenet's version, compared with a contract's `requires-tenet`.
const CURRENT: (u64, u64, u64) = (
    parse_decimal(env!("CARGO_PKG_VERSION_MAJOR")),
    parse_decimal(env!("CARGO_PKG_VERSION_MINOR")),
    parse_decimal(env!("CARGO_PKG_VERSION_PATCH")),
);

/// Read a Cargo version part at compile time.
#[expect(
    clippy::panic,
    reason = "only constant evaluation calls this, on the decimal version parts Cargo sets, so a \
              failure stops the build"
)]
const fn parse_decimal(digits: &str) -> u64 {
    match u64::from_str_radix(digits, 10) {
        Ok(number) => number,
        Err(_) => panic!("each Cargo version part is a decimal number"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    /// Checked first, through [`Requirement`].
    #[serde(rename = "requires-tenet")]
    _requires_tenet: Option<IgnoredAny>,
    message: String,
    name: String,
    paths: Option<Vec<String>>,
}

/// The oldest Tenet that reads a contract, like Cargo's `rust-version`. It is read before the
/// other keys, so a newer contract's keys fail with the version they need.
#[derive(Deserialize)]
struct Requirement {
    #[serde(rename = "requires-tenet")]
    requires_tenet: Option<String>,
}

/// Parse and validate a contract document read from `path`.
#[inline]
pub fn parse(path: &Path, document: &str) -> Result<Contract, DocumentError> {
    parse_inner(path, document).map_err(|refusal| {
        match refusal {
            Refusal::Located(error) => error,
            Refusal::Unlocated(problem) => {
                DocumentError {
                    line: 1,
                    path: path.to_owned(),
                    problem,
                }
            },
        }
    })
}

/// The scope directory and number of a contract at `path`,
/// `<scope>/.contracts/NNN-name/CONTRACT.md`.
fn parse_location(path: &Path) -> Result<(&Path, u32), DocumentProblem> {
    let folder = path.parent().ok_or(DocumentProblem::NoDirectory)?;
    let parent = folder.parent().ok_or(DocumentProblem::NoContracts)?;
    let scope = parent
        .parent()
        .filter(|_| {
            parent.file_name().is_some_and(|name| name == ".contracts")
                && path.file_name().is_some_and(|name| name == "CONTRACT.md")
        })
        .ok_or(DocumentProblem::Layout)?;
    let directory = folder
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(DocumentProblem::DirectoryName)?;
    let (digits, suffix) = directory.split_once('-').ok_or(DocumentProblem::NoNumber)?;
    if digits.len() < 3 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(DocumentProblem::ShortNumber);
    }
    let number: u32 = digits.parse().map_err(DocumentProblem::Number)?;
    if number == 0 || !is_kebab(suffix) {
        return Err(DocumentProblem::DirectoryFormat);
    }
    Ok((scope, number))
}

/// Read the frontmatter keys, refusing a `requires-tenet` newer than this Tenet before the others.
fn parse_metadata(path: &Path, yaml: &str) -> Result<Metadata, Refusal> {
    let requirement: Requirement =
        serde_yaml::from_str(yaml).map_err(|error| convert_yaml_error(path, error))?;
    if let Some(required) = requirement.requires_tenet {
        check_supported(&required).map_err(|problem| {
            DocumentError {
                line: find_key_line(yaml, "requires-tenet:"),
                path: path.to_owned(),
                problem,
            }
        })?;
    }
    let metadata: Metadata =
        serde_yaml::from_str(yaml).map_err(|error| convert_yaml_error(path, error))?;
    if !is_kebab(&metadata.name) {
        return Err(DocumentProblem::ContractName.into());
    }
    if metadata.message.trim().is_empty() {
        return Err(DocumentProblem::NoMessage.into());
    }
    Ok(metadata)
}

/// The trimmed rules in `span` of `document`, with their 1-based first line and byte range.
#[expect(
    clippy::arithmetic_side_effects,
    clippy::string_slice,
    reason = "pulldown-cmark section spans fall on character boundaries of the document, which \
              parse caps at `DOCUMENT_BYTES`, so offset sums and the line count stay small"
)]
fn extract_rules(document: &str, span: Range<usize>) -> (String, usize, Range<usize>) {
    let section = &document[span.clone()];
    let rules = section.trim().to_owned();
    let start = span.start + section.len() - section.trim_start().len();
    let line = document[..start]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    let end = start + rules.len();
    (rules, line, start..end)
}

#[expect(
    clippy::string_slice,
    reason = "frontmatter returns a body offset at a line start of the document"
)]
fn parse_inner(path: &Path, document: &str) -> Result<Contract, Refusal> {
    if document.len() > DOCUMENT_BYTES {
        return Err(DocumentProblem::TooLarge.into());
    }
    if let Some(line) = find_conflict_marker(document) {
        return Err(DocumentError {
            line,
            path: path.to_owned(),
            problem: DocumentProblem::ConflictMarker,
        }
        .into());
    }
    let (scope, number) = parse_location(path)?;
    let (yaml, offset) = split_frontmatter(document).map_err(DocumentProblem::Frontmatter)?;
    let metadata = parse_metadata(path, yaml)?;
    let paths = metadata
        .paths
        .as_deref()
        .map(Patterns::new)
        .transpose()
        .map_err(|problem| {
            DocumentError {
                line: find_key_line(yaml, "paths:"),
                path: path.to_owned(),
                problem,
            }
        })?;
    let markdown = markdown::parse(&document[offset..], offset)?;
    if markdown.body.is_empty() {
        return Err(DocumentProblem::NoBody.into());
    }
    let section = markdown
        .sections
        .get("Rules")
        .cloned()
        .unwrap_or(offset..document.len());
    let (rules, rules_line, rules_span) = extract_rules(document, section);
    Ok(Contract {
        body: markdown.body,
        document: document.to_owned(),
        hash: blake3::hash(document.as_bytes()).to_hex().to_string(),
        message: metadata.message,
        name: metadata.name,
        number,
        path: path.to_owned(),
        paths,
        rules,
        rules_line,
        rules_span,
        scope: scope.to_owned(),
    })
}

/// Split a document into its YAML frontmatter and the offset where its body starts.
#[expect(
    clippy::arithmetic_side_effects,
    clippy::string_slice,
    reason = "the offsets sum whole lines of `document`, so they fall on its character boundaries \
              and never exceed its length"
)]
pub(crate) fn split_frontmatter(document: &str) -> Result<(&str, usize), FrontmatterError> {
    let mut lines = document.split_inclusive('\n');
    let first = lines
        .next()
        .filter(|line| line.ends_with('\n') && line.trim_end() == "---")
        .ok_or(FrontmatterError::Missing)?;
    let mut end = first.len();
    for line in lines {
        if line.trim_end() == "---" {
            return Ok((&document[first.len()..end], end + line.len()));
        }
        end += line.len();
    }
    Err(FrontmatterError::Unclosed)
}

/// The 1-based document line of the frontmatter line that starts with `key`, or 1 when none does.
/// The frontmatter starts on the document's line 2.
pub(crate) fn find_key_line(yaml: &str, key: &str) -> usize {
    (2..)
        .zip(yaml.lines())
        .find_map(|(number, line)| line.starts_with(key).then_some(number))
        .unwrap_or(1)
}

/// A frontmatter parse error, located in the document; the YAML starts on the document's line 2.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "serde_yaml reports a line of a document held in memory, far below `usize::MAX`"
)]
pub(crate) fn convert_yaml_error(path: &Path, error: serde_yaml::Error) -> DocumentError {
    DocumentError {
        line: error.location().map_or(2, |location| location.line() + 1),
        path: path.to_owned(),
        problem: error.into(),
    }
}

/// Refuse a `requires-tenet` version newer than this Tenet.
fn check_supported(required: &str) -> Result<(), DocumentProblem> {
    let version = parse_version(required).ok_or_else(|| {
        DocumentProblem::RequirementFormat {
            required: required.to_owned(),
        }
    })?;
    if version > CURRENT {
        return Err(DocumentProblem::NewerTenet {
            required: required.to_owned(),
        });
    }
    Ok(())
}

/// Parse a release version `X.Y.Z` written in decimal digits.
#[must_use]
#[inline]
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.split('.');
    let mut number = || {
        parts
            .next()
            .filter(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))?
            .parse()
            .ok()
    };
    let version = (number()?, number()?, number()?);
    parts.next().is_none().then_some(version)
}

/// Replace the frontmatter `name` value, leaving every other byte unchanged.
#[expect(
    clippy::string_slice,
    reason = "`name_value` returns a range that falls on character boundaries of `document`"
)]
#[inline]
pub fn with_name(document: &str, name: &str) -> Result<String, FrontmatterError> {
    let value = name_value(document)?;
    Ok(format!(
        "{}{name}{}",
        &document[..value.start],
        &document[value.end..]
    ))
}

/// Return the frontmatter `name` value without validating the rest of the document.
#[expect(
    clippy::string_slice,
    reason = "`name_value` returns a range that falls on character boundaries of `document`"
)]
#[inline]
pub fn name_of(document: &str) -> Result<&str, FrontmatterError> {
    Ok(&document[name_value(document)?])
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "the offsets sum whole lines of `document`, so they never exceed its length"
)]
fn name_value(document: &str) -> Result<Range<usize>, FrontmatterError> {
    let mut lines = document.split_inclusive('\n');
    let first = lines.next().unwrap_or_default();
    if first.trim_end() != "---" {
        return Err(FrontmatterError::Missing);
    }
    let mut offset = first.len();
    for line in lines {
        if line.trim_end() == "---" {
            break;
        }
        if let Some(rest) = line.strip_prefix("name:") {
            let start = offset + "name:".len() + rest.len() - rest.trim_start().len();
            return Ok(start..start + rest.trim().len());
        }
        offset += line.len();
    }
    Err(FrontmatterError::NoName)
}

/// Return the 1-based line of the first Git conflict marker, if any.
#[must_use]
#[inline]
pub fn find_conflict_marker(text: &str) -> Option<usize> {
    (1..).zip(text.lines()).find_map(|(number, line)| {
        ["<<<<<<<", ">>>>>>>"]
            .iter()
            .any(|marker| line == *marker || line.starts_with(&format!("{marker} ")))
            .then_some(number)
    })
}

/// Return whether `value` is lowercase letters and digits joined by single hyphens.
#[must_use]
#[inline]
pub fn is_kebab(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}
