use tenet_engine::{Status, Summary};

pub(super) fn status(state: Status) -> &'static str {
    match state {
        Status::Pass => "PASS",
        Status::Fail => "FAIL",
        Status::NotApplicable => "EXCLUDED",
        Status::Uncertain => "UNRESOLVED",
        Status::Error => "ERROR",
        Status::Incomplete => "INCOMPLETE",
    }
}
pub(super) fn status_name(state: Status) -> &'static str {
    match state {
        Status::Pass => "pass",
        Status::Fail => "fail",
        Status::NotApplicable => "not_applicable",
        Status::Uncertain => "uncertain",
        Status::Error => "error",
        Status::Incomplete => "incomplete",
    }
}
pub(super) fn suite(s: &Summary, evaluation: bool) -> &'static str {
    if s.errors > 0 {
        "ERROR"
    } else if s.incomplete > 0 {
        "INCOMPLETE"
    } else if evaluation {
        if s.examples_failed > 0 {
            "FAIL"
        } else if s.examples_passed > 0 {
            "PASS"
        } else {
            "EMPTY"
        }
    } else if s.failed > 0 {
        "FAIL"
    } else if s.uncertain > 0 {
        "UNCERTAIN"
    } else if s.passed > 0 {
        "PASS"
    } else {
        "EMPTY"
    }
}
pub(super) fn counts(s: &Summary, evaluation: bool) -> String {
    let entries = if evaluation {
        vec![
            (s.examples_passed, "passed"),
            (s.examples_failed, "failed"),
            (s.errors, "errors"),
            (s.incomplete, "incomplete"),
        ]
    } else {
        vec![
            (s.passed, "passed"),
            (s.failed, "failed"),
            (s.uncertain, "uncertain"),
            (s.not_applicable, "not applicable"),
            (s.errors, "errors"),
            (s.incomplete, "incomplete"),
        ]
    };
    let counts = entries
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, state)| format!("{count} {state}"))
        .collect::<Vec<_>>()
        .join(" | ");
    if counts.is_empty() {
        "0 completed".into()
    } else {
        counts
    }
}

pub(super) fn paint(state: &str) -> String {
    let styled = console::style(state).for_stderr();
    match state.to_ascii_uppercase().as_str() {
        "PASS" | "VERIFIED" | "PRESERVED" => styled.green().to_string(),
        "FAIL" | "FAILED" | "ERROR" => styled.red().bold().to_string(),
        "UNCERTAIN" | "UNRESOLVED" | "INCOMPLETE" => styled.yellow().to_string(),
        _ => styled.dim().to_string(),
    }
}

pub(super) fn marker(state: &str) -> String {
    let symbol = match state {
        "PASS" | "VERIFIED" | "PRESERVED" => "✓",
        "FAIL" | "FAILED" | "ERROR" => "×",
        "UNCERTAIN" | "UNRESOLVED" | "INCOMPLETE" => "?",
        _ => "−",
    };
    match state {
        "PASS" | "VERIFIED" | "PRESERVED" => {
            console::style(symbol).for_stderr().green().to_string()
        }
        "FAIL" | "FAILED" | "ERROR" => console::style(symbol).for_stderr().red().to_string(),
        "UNCERTAIN" | "UNRESOLVED" | "INCOMPLETE" => {
            console::style(symbol).for_stderr().yellow().to_string()
        }
        _ => console::style(symbol).for_stderr().dim().to_string(),
    }
}

pub(super) fn contract_counts(
    states: &std::collections::BTreeMap<&str, usize>,
    total: usize,
) -> String {
    let counts = states
        .iter()
        .map(|(state, count)| format!("{count} {}", paint(&state.to_lowercase())))
        .collect::<Vec<_>>()
        .join(" | ");
    if counts.is_empty() {
        format!("0 completed ({total})")
    } else {
        format!("{counts} ({total})")
    }
}
