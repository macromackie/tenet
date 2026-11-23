use serde_json::json;
use tenet_checks::behavior::{Finding, Outcome};

use super::{DiagnosticBlock, block::split_wrapped_lines, format_display, write_diagnostic_block};
use crate::generated::summarize_generation;

fn sample_finding(check: &str, message: &str, requested: bool, reused: bool) -> Finding {
    Finding {
        check: check.into(),
        contract: "options".into(),
        cost_usd: requested.then_some(0.003_f64),
        explanation_cost_usd: requested.then_some(0.0005_f64),
        explanation_requests: requested.then_some(1),
        generation_requests: requested.then_some(2),
        message: message.into(),
        outcome: Outcome::Passed,
        pin_reused: reused.then_some(true),
        repro: None,
        requested,
        reused,
        severity: None,
        subject: None,
    }
}

#[test]
fn summarize_generation_counts_repair_attempts_without_double_counting_reused_rows() {
    let rows = vec![
        sample_finding("zero-timeout", "kept explicit zero", true, false),
        sample_finding("default-read", "reused check", false, true),
    ];
    let summary = summarize_generation(&rows);
    assert_eq!(summary.get("requests"), Some(&json!(2_usize)));
    assert_eq!(summary.get("returned_cost_usd"), Some(&json!(0.003_f64)));
    assert_eq!(summary.get("explanation_requests"), Some(&json!(1_usize)));
    assert_eq!(
        summary.get("explanation_cost_usd"),
        Some(&json!(0.0005_f64))
    );

    let zero_summary = summarize_generation(&[]);
    assert_eq!(zero_summary.get("requests"), Some(&json!(0_usize)));
    assert_eq!(
        zero_summary
            .get("returned_cost_usd")
            .map(ToString::to_string),
        Some(String::from("0.0"))
    );
    assert_eq!(
        zero_summary.get("explanation_requests"),
        Some(&json!(0_usize))
    );
    assert_eq!(
        zero_summary
            .get("explanation_cost_usd")
            .map(ToString::to_string),
        Some(String::from("0.0"))
    );
}

#[test]
fn format_display_preserves_quotes_and_strips_controls() {
    let raw = "caller's \"list\" mutated\n\tin \x1b[31mfoo.py\x1b[0m\u{202e}!";
    assert_eq!(format_display(raw), "caller's \"list\" mutated in foo.py!");
}

#[test]
fn split_wrapped_lines_breaks_long_text_on_word_boundaries() {
    let lines = split_wrapped_lines(
        "normalize_all mutates the caller's list in place instead of returning a new copy.",
        32,
    );
    assert_eq!(
        lines,
        vec![
            "normalize_all mutates the",
            "caller's list in place instead",
            "of returning a new copy.",
        ]
    );
}

#[test]
fn write_diagnostic_block_renders_structured_gutter_and_help() {
    let mut bytes = Vec::new();
    let write_result = write_diagnostic_block(
        &mut bytes,
        &DiagnosticBlock {
            help: Some("tenet show borrowed-inputs"),
            label: "FAIL",
            location: Some("src/text.py"),
            message: "caller's list mutated in place",
            meta: Some(" \u{b7} violation 98%"),
            title: "borrowed-inputs/mutation",
        },
    );
    write_result.unwrap();
    let plain = console::strip_ansi_codes(&String::from_utf8_lossy(&bytes)).into_owned();
    assert!(plain.starts_with('\n'), "{plain:?}");
    assert!(
        plain.contains("FAIL  borrowed-inputs/mutation"),
        "{plain:?}"
    );
    assert!(plain.contains("\u{256d}\u{2500}[src/text.py]"), "{plain:?}");
    assert!(
        plain.contains("\u{2502} caller's list mutated in place"),
        "{plain:?}"
    );
    assert!(
        plain.contains("\u{2502} = help: tenet show borrowed-inputs"),
        "{plain:?}"
    );
    assert!(plain.contains("\u{2570}\u{2500}"), "{plain:?}");
}
