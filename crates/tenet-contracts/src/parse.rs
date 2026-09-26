use std::{error::Error, fmt, path::Path};

use anyhow::{Context, Result, ensure};
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
        number > 0 && kebab(suffix),
        "contract directory must be NNN-kebab-case with a positive number"
    );
    let mut lines = document.split_inclusive('\n');
    ensure!(
        lines.next().is_some_and(|line| line.trim_end() == "---"),
        "contract requires YAML frontmatter"
    );
    let start = document.find('\n').context("missing frontmatter")? + 1;
    let mut end = start;
    let mut body = None;
    for line in lines {
        if line.trim_end() == "---" {
            body = Some(end + line.len());
            break;
        }
        end += line.len();
    }
    let offset = body.context("unclosed YAML frontmatter")?;
    let metadata: Metadata =
        serde_yaml::from_str(&document[start..end]).map_err(|error| DocumentError {
            path: path.to_owned(),
            line: error.location().map_or(2, |loc| loc.line() + 1),
            message: error.to_string(),
        })?;
    ensure!(kebab(&metadata.name), "name must be lowercase kebab-case");
    ensure!(
        !metadata.message.trim().is_empty(),
        "message must not be empty"
    );
    let markdown = markdown::parse(&document[offset..], offset, document)?;
    let rules_span = markdown
        .sections
        .get("Rules")
        .cloned()
        .unwrap_or(offset..document.len());
    let rules = document[rules_span.clone()].trim().to_owned();
    ensure!(!markdown.body.is_empty(), "contract body must not be empty");
    let applies_to = markdown
        .sections
        .get("Applies to")
        .map(|r| document[r.clone()].trim().to_owned());
    ensure!(
        applies_to.as_ref().is_none_or(|s| !s.is_empty()),
        "## Applies to must not be empty"
    );
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
        applies_to,
        examples: markdown.examples,
        rules_line: document[..rules_start]
            .bytes()
            .filter(|b| *b == b'\n')
            .count()
            + 1,
        rules_span: rules_start..rules_end,
        hash: blake3::hash(document.as_bytes()).to_hex().to_string(),
        document: document.to_owned(),
    })
}

fn kebab(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}
