use crate::output;

const BIDI_CONTROLS: &[char] = &[
    '\u{061C}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}',
    '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
];
const WHITESPACE_CONTROLS: &[char] = &['\n', '\r', '\t'];

pub(crate) fn format_dim(value: &str) -> String {
    output::format_styled(value).dim().to_string()
}

/// Clean untrusted text for terminal display without escaping quotes or apostrophes.
pub(crate) fn format_display(value: &str) -> String {
    let stripped = console::strip_ansi_codes(value);
    let cleaned: String = stripped
        .chars()
        .filter_map(|character| {
            if WHITESPACE_CONTROLS.contains(&character) {
                Some(' ')
            } else if character.is_control() || BIDI_CONTROLS.contains(&character) {
                None
            } else {
                Some(character)
            }
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    clippy::float_arithmetic,
    reason = "Elapsed milliseconds are rounded for display and stay below 2^52."
)]
pub(crate) fn format_duration(elapsed_ms: u128) -> String {
    if elapsed_ms < 1_000 {
        format!("{elapsed_ms}ms")
    } else {
        format!("{:.2}s", elapsed_ms as f64 / 1_000.0_f64)
    }
}

pub(crate) fn format_marker(state: &str) -> String {
    let symbol = |symbol| output::format_styled(symbol);
    match state {
        "PASS" | "VERIFIED" | "PRESERVED" | "CLEAR" => {
            symbol("\u{2713}").green().bold().to_string()
        },
        "REVIEWED" => symbol("\u{2713}").cyan().bold().to_string(),
        "FAIL" | "FAILED" | "ERROR" => symbol("\u{d7}").red().bold().to_string(),
        "REVIEW" | "UNCERTAIN" | "UNRESOLVED" | "INCOMPLETE" | "PENDING" => {
            symbol("?").yellow().bold().to_string()
        },
        _ => symbol("\u{2212}").dim().to_string(),
    }
}

pub(crate) fn format_painted(state: &str) -> String {
    let styled = output::format_styled(state);
    match state.to_ascii_uppercase().as_str() {
        "PASS" | "VERIFIED" | "PRESERVED" | "CLEAR" => styled.green().bold().to_string(),
        "PASSED" | "CLEAR IN SELECTED FILES" => styled.green().to_string(),
        "REVIEWED" => styled.cyan().bold().to_string(),
        "FAIL" | "FAILED" | "ERROR" => styled.red().bold().to_string(),
        "REVIEW" | "PENDING" => styled.yellow().bold().to_string(),
        "FOR REVIEW" | "UNCERTAIN" | "UNRESOLVED" | "INCOMPLETE" => styled.yellow().to_string(),
        _ => styled.dim().to_string(),
    }
}

pub(crate) fn quote_argument(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-./:".contains(character))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}
