//! The program's only handles to standard output and standard error.
//!
//! A command writes its result, such as a report or a JSON document, to [`open_stdout`], and every
//! other line, such as a warning, an error, progress, or a check's human report, to [`open_stderr`]
//! or the [`open_terminal`] on it. A function that receives one of these writers may write to it.

use std::io::{self, Write};

use console::{StyledObject, Term};
use serde_json::Value;

/// Standard output, which carries only the command's result.
#[expect(
    clippy::disallowed_methods,
    reason = "this module holds the program's only handle to standard output"
)]
pub(crate) fn open_stdout() -> io::Stdout {
    io::stdout()
}

/// Standard error, which carries every line that is not the result.
#[expect(
    clippy::disallowed_methods,
    reason = "this module holds the program's only handle to standard error"
)]
pub(crate) fn open_stderr() -> io::Stderr {
    io::stderr()
}

/// The terminal on standard error that the human reporters write and redraw.
#[expect(
    clippy::disallowed_methods,
    reason = "this module holds the program's only handle to standard error"
)]
pub(crate) fn open_terminal() -> Term {
    Term::buffered_stderr()
}

/// Whether standard error emits ANSI styling.
pub(crate) fn is_stderr_styled() -> bool {
    console::colors_enabled_stderr()
}

/// Active terminal width in columns when standard error is attached to a terminal.
pub(crate) fn read_terminal_columns() -> Option<u16> {
    open_terminal().size_checked().map(|(_, columns)| columns)
}

/// Text styled for standard error, colored only when standard error shows colors.
pub(crate) fn format_styled<Value>(value: Value) -> StyledObject<Value> {
    console::style(value).for_stderr()
}

/// Write one JSON document as a line, with `version` set to its format's schema version.
#[expect(
    clippy::indexing_slicing,
    reason = "Every document is a JSON object, so the key assignment inserts a field."
)]
pub(crate) fn write_document(
    out: &mut impl Write,
    version: u64,
    mut value: Value,
) -> io::Result<()> {
    value["version"] = version.into();
    writeln!(out, "{value}")
}
