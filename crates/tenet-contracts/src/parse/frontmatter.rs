use core::ops::Range;
use std::path::Path;

use serde::de::DeserializeOwned;

use crate::document::{DocumentError, FrontmatterError};

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

/// Read typed YAML without coercing scalars to strings, locating a refusal in the whole document.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "the parser reports a line of a document held in memory, far below `usize::MAX`"
)]
pub(crate) fn parse_frontmatter<Frontmatter>(
    path: &Path,
    yaml: &str,
) -> Result<Frontmatter, DocumentError>
where
    Frontmatter: DeserializeOwned,
{
    serde_saphyr::from_str_with_options(
        yaml,
        serde_saphyr::options! { no_schema: true, strict_booleans: true },
    )
    .map_err(|error| {
        DocumentError {
            line: error.location().map_or(2, |location| {
                usize::try_from(location.line()).map_or(2, |line| line + 1)
            }),
            path: path.to_owned(),
            problem: Box::new(error).into(),
        }
    })
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
