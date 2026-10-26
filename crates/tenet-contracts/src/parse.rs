use std::{error::Error, fmt, ops::Range, path::Path};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use crate::{Contract, markdown};

#[derive(Debug)]
pub struct DocumentError {
    pub path: std::path::PathBuf,
    pub line: usize,
    pub message: String,
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.path.display(), self.line, self.message)
    }
}
impl Error for DocumentError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    name: String,
    message: String,
    paths: Option<Vec<String>>,
    /// Checked first, through [`Requirement`].
    #[serde(rename = "requires-tenet")]
    _requires_tenet: Option<serde::de::IgnoredAny>,
}

/// The oldest Tenet that reads a contract, like Cargo's `rust-version`. It is read before the
/// other keys, so a newer contract's keys fail with the version they need.
#[derive(Deserialize)]
struct Requirement {
    #[serde(rename = "requires-tenet")]
    requires_tenet: Option<String>,
}

pub fn parse(path: &Path, document: &str) -> Result<Contract> {
    parse_inner(path, document).map_err(|error| {
        if error.downcast_ref::<DocumentError>().is_some() {
            return error;
        }
        DocumentError {
            path: path.to_owned(),
            line: 1,
            message: error.to_string(),
        }
        .into()
    })
}

fn parse_inner(path: &Path, document: &str) -> Result<Contract> {
    ensure!(document.len() <= 65536, "contract exceeds 65536 bytes");
    if let Some(line) = conflict_marker(document) {
        return Err(DocumentError {
            path: path.to_owned(),
            line,
            message: "unresolved merge conflict marker".into(),
        }
        .into());
    }
    let folder = path
        .parent()
        .context("contract requires a numbered directory")?;
    let parent = folder.parent().context("contract requires .contracts")?;
    ensure!(
        parent.file_name().is_some_and(|p| p == ".contracts")
            && path.file_name().is_some_and(|p| p == "CONTRACT.md"),
        "expected .contracts/NNN-name/CONTRACT.md"
    );
    let directory = folder
        .file_name()
        .and_then(|n| n.to_str())
        .context("invalid contract directory")?;
    let (digits, suffix) = directory
        .split_once('-')
        .context("contract directory requires NNN-name")?;
    ensure!(
        digits.len() >= 3 && digits.bytes().all(|b| b.is_ascii_digit()),
        "contract number needs at least three digits"
    );
    let number: u32 = digits.parse()?;
    ensure!(
        number > 0 && is_kebab(suffix),
        "contract directory must be NNN-kebab-case with a positive number"
    );
    let (yaml, offset) = frontmatter(document)?;
    let requirement: Requirement =
        serde_yaml::from_str(&document[yaml.clone()]).map_err(|error| yaml_error(path, &error))?;
    if let Some(required) = requirement.requires_tenet {
        supported(&required).map_err(|error| DocumentError {
            path: path.to_owned(),
            line: key_line(document, &yaml, "requires-tenet:"),
            message: error.to_string(),
        })?;
    }
    let metadata: Metadata =
        serde_yaml::from_str(&document[yaml.clone()]).map_err(|error| yaml_error(path, &error))?;
    ensure!(
        is_kebab(&metadata.name),
        "name must be lowercase kebab-case"
    );
    ensure!(
        !metadata.message.trim().is_empty(),
        "message must not be empty"
    );
    let paths = metadata
        .paths
        .as_deref()
        .map(crate::scope::Patterns::new)
        .transpose()
        .map_err(|error| DocumentError {
            path: path.to_owned(),
            line: key_line(document, &yaml, "paths:"),
            message: error.to_string(),
        })?;
    let markdown = markdown::parse(&document[offset..], offset, document)?;
    let rules_span = markdown
        .sections
        .get("Rules")
        .cloned()
        .unwrap_or(offset..document.len());
    let rules = document[rules_span.clone()].trim().to_owned();
    ensure!(!markdown.body.is_empty(), "contract body must not be empty");
    let rules_start = rules_span.start + document[rules_span.clone()].len()
        - document[rules_span.clone()].trim_start().len();
    let rules_end = rules_start + rules.len();
    Ok(Contract {
        name: metadata.name,
        message: metadata.message,
        number,
        path: path.to_owned(),
        scope: parent.parent().context("missing scope")?.to_owned(),
        rules,
        body: markdown.body,
        rules_line: document[..rules_start]
            .bytes()
            .filter(|b| *b == b'\n')
            .count()
            + 1,
        rules_span: rules_start..rules_end,
        hash: blake3::hash(document.as_bytes()).to_hex().to_string(),
        document: document.to_owned(),
        paths,
    })
}

/// Split a document into the byte range of its YAML frontmatter and the offset where its body starts.
pub(crate) fn frontmatter(document: &str) -> Result<(Range<usize>, usize)> {
    let mut lines = document.split_inclusive('\n');
    ensure!(
        lines.next().is_some_and(|line| line.trim_end() == "---"),
        "missing YAML frontmatter"
    );
    let start = document.find('\n').context("missing YAML frontmatter")? + 1;
    let mut end = start;
    for line in lines {
        if line.trim_end() == "---" {
            return Ok((start..end, end + line.len()));
        }
        end += line.len();
    }
    bail!("unclosed YAML frontmatter")
}

/// The 1-based line of the frontmatter line that starts with `key`, or 1 when none does.
pub(crate) fn key_line(document: &str, yaml: &Range<usize>, key: &str) -> usize {
    document[..yaml.end]
        .lines()
        .position(|line| line.starts_with(key))
        .map_or(1, |index| index + 1)
}

/// A frontmatter parse error, located in the document; the YAML starts on the document's line 2.
pub(crate) fn yaml_error(path: &Path, error: &serde_yaml::Error) -> DocumentError {
    DocumentError {
        path: path.to_owned(),
        line: error.location().map_or(2, |location| location.line() + 1),
        message: error.to_string(),
    }
}

/// Refuse a `requires-tenet` version newer than this Tenet.
fn supported(required: &str) -> Result<()> {
    let version = parse_version(required).with_context(|| {
        format!("requires-tenet must be a version such as 0.11.0, not {required}")
    })?;
    let current = (
        env!("CARGO_PKG_VERSION_MAJOR").parse()?,
        env!("CARGO_PKG_VERSION_MINOR").parse()?,
        env!("CARGO_PKG_VERSION_PATCH").parse()?,
    );
    ensure!(
        version <= current,
        "requires Tenet {required}; this is Tenet {}",
        env!("CARGO_PKG_VERSION")
    );
    Ok(())
}

/// Parse a release version `X.Y.Z` written in decimal digits.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.split('.');
    let mut number = || {
        parts
            .next()
            .filter(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))?
            .parse()
            .ok()
    };
    let version = (number()?, number()?, number()?);
    parts.next().is_none().then_some(version)
}

/// Replace the frontmatter `name` value, leaving every other byte unchanged.
pub fn with_name(document: &str, name: &str) -> Result<String> {
    let value = name_value(document)?;
    Ok(format!(
        "{}{name}{}",
        &document[..value.start],
        &document[value.end..]
    ))
}

/// Return the frontmatter `name` value without validating the rest of the document.
pub fn name_of(document: &str) -> Result<&str> {
    Ok(&document[name_value(document)?])
}

fn name_value(document: &str) -> Result<Range<usize>> {
    let mut lines = document.split_inclusive('\n');
    let first = lines.next().unwrap_or_default();
    ensure!(first.trim_end() == "---", "missing YAML frontmatter");
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
    bail!("frontmatter has no name")
}

/// Return the 1-based line of the first Git conflict marker, if any.
pub fn conflict_marker(text: &str) -> Option<usize> {
    text.lines()
        .position(|line| {
            ["<<<<<<<", ">>>>>>>"]
                .iter()
                .any(|marker| line == *marker || line.starts_with(&format!("{marker} ")))
        })
        .map(|index| index + 1)
}

pub fn is_kebab(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}
