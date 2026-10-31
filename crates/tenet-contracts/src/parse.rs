mod frontmatter;
mod version;

use core::ops::Range;
use std::path::Path;

pub(crate) use frontmatter::{find_key_line, parse_frontmatter, split_frontmatter};
pub use frontmatter::{name_of, with_name};
use serde::{Deserialize, de::IgnoredAny};
use version::check_supported;
pub use version::parse_version;

use crate::{
    Contract,
    document::{DOCUMENT_BYTES, DocumentError, DocumentProblem},
    markdown,
    scope::Patterns,
};

/// A contract failure before [`parse`] locates it: on its line, or on line 1.
#[derive(Debug, thiserror::Error)]
enum Refusal {
    #[error(transparent)]
    Located(#[from] DocumentError),
    #[error(transparent)]
    Unlocated(#[from] DocumentProblem),
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
    let requirement: Requirement = parse_frontmatter(path, yaml)?;
    if let Some(required) = requirement.requires_tenet {
        check_supported(&required).map_err(|problem| {
            DocumentError {
                line: find_key_line(yaml, "requires-tenet:"),
                path: path.to_owned(),
                problem,
            }
        })?;
    }
    let metadata: Metadata = parse_frontmatter(path, yaml)?;
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
