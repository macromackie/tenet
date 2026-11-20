use std::io::{self, Write};

use super::style::{format_dim, format_display, format_marker, format_painted};
use crate::output;

/// Default maximum total line width when stderr is not attached to a terminal.
const DEFAULT_COLUMNS: usize = 88;
/// Width of the left gutter (`"    │ "`) before wrapped diagnostic body text.
const GUTTER_COLUMNS: usize = 6;
/// Maximum wrapped text width so long diagnostics stay easy to scan on wide terminals.
const MAX_CONTENT_COLUMNS: usize = 82;
/// Minimum wrapped text width so narrow terminals still wrap cleanly without degenerating.
const MIN_CONTENT_COLUMNS: usize = 40;

/// Structured diagnostic block rendered with Ruff/`ty`-style gutters and vertical breathing room.
pub(crate) struct DiagnosticBlock<'text> {
    pub help: Option<&'text str>,
    pub label: &'text str,
    pub location: Option<&'text str>,
    pub message: &'text str,
    pub meta: Option<&'text str>,
    pub title: &'text str,
}

/// Measure a comfortable body-text width from the active stderr terminal, clamped to readable
/// bounds.
fn read_content_width() -> usize {
    let columns = output::read_terminal_columns().map_or(DEFAULT_COLUMNS, usize::from);
    columns
        .saturating_sub(GUTTER_COLUMNS)
        .clamp(MIN_CONTENT_COLUMNS, MAX_CONTENT_COLUMNS)
}

/// Split sanitized text into word-wrapped lines that fit within `max_width` characters.
pub(crate) fn split_wrapped_lines(text: &str, max_width: usize) -> Vec<String> {
    let width = max_width.max(1);
    let cleaned = format_display(text);
    if cleaned.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_len = 0_usize;
    for word in cleaned.split_whitespace() {
        let word_len = word.chars().count();
        if current.is_empty() {
            current.push_str(word);
            current_len = word_len;
        } else if current_len.saturating_add(1).saturating_add(word_len) <= width {
            current.push(' ');
            current.push_str(word);
            current_len = current_len.saturating_add(1).saturating_add(word_len);
        } else {
            lines.push(current);
            current = String::from(word);
            current_len = word_len;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Write a vertically separated diagnostic block with a clickable location pointer, aligned
/// gutter-wrapped body text, and an optional help footer.
pub(crate) fn write_diagnostic_block(
    out: &mut impl Write,
    block: &DiagnosticBlock<'_>,
) -> io::Result<()> {
    let meta = block
        .meta
        .filter(|value| !value.is_empty())
        .map(format_dim)
        .unwrap_or_default();
    writeln!(out)?;
    writeln!(
        out,
        "  {} {}  {}{meta}",
        format_marker(block.label),
        format_painted(block.label),
        format_display(block.title),
    )?;
    if let Some(location) = block.location.filter(|value| !value.is_empty()) {
        writeln!(
            out,
            "    {}{}{}",
            format_dim("\u{256d}\u{2500}["),
            output::format_styled(format_display(location)).cyan(),
            format_dim("]"),
        )?;
    } else {
        writeln!(out, "    {}", format_dim("\u{256d}\u{2500}"))?;
    }
    let width = read_content_width();
    for line in split_wrapped_lines(block.message, width) {
        writeln!(out, "    {} {line}", format_dim("\u{2502}"))?;
    }
    if let Some(help) = block.help.filter(|value| !value.is_empty()) {
        writeln!(
            out,
            "    {} {} {}",
            format_dim("\u{2502}"),
            output::format_styled("= help:").cyan().bold(),
            format_display(help),
        )?;
    }
    writeln!(out, "    {}", format_dim("\u{2570}\u{2500}"))
}
