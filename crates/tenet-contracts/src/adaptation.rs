//! The final `## Local adaptations` section in which a project keeps its exceptions and additions
//! to an installed library contract.

use crate::{document::FrontmatterError, markdown, parse::split_frontmatter};

/// The heading of the section that holds a project's local adaptations.
const HEADING: &str = "Local adaptations";

/// An installed contract split at its final `## Local adaptations` section.
#[derive(Debug)]
pub struct Adaptations<'document> {
    /// The section, from its heading to the end of the document.
    pub section: &'document str,
    /// The text above the section, ending with one newline, as a library release ships it.
    pub upstream: String,
}

/// Split an installed contract at its final `## Local adaptations` section, dropping the blank
/// lines above the heading. `None` when the contract's last section has another heading.
#[expect(
    clippy::string_slice,
    reason = "The frontmatter ends and pulldown-cmark headings start on character boundaries of \
              `document`."
)]
#[inline]
pub fn split_adaptations(document: &str) -> Result<Option<Adaptations<'_>>, FrontmatterError> {
    let (_, offset) = split_frontmatter(document)?;
    Ok(
        markdown::find_final_section(&document[offset..], offset, HEADING).map(|start| {
            Adaptations {
                section: &document[start..],
                upstream: format!("{}\n", document[..start].trim_end_matches('\n')),
            }
        }),
    )
}

/// Append a local adaptations section to a library release's text, after one blank line, so that
/// [`split_adaptations`] returns both unchanged.
#[must_use]
#[inline]
pub fn append_adaptations(upstream: &str, section: &str) -> String {
    format!("{}\n\n{section}", upstream.trim_end_matches('\n'))
}
