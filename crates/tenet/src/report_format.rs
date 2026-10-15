use tenet_engine::{
    ContextSummary, ContractState, ContractStatus, Coverage, MAX_FILE_BYTES, Omitted, Outcome,
    Pending, RequestFailure, Status, Summary, review::Answer,
};

pub(super) fn status(state: Status) -> &'static str {
    match state {
        Status::Pass => "PASS",
        Status::Fail => "FAIL",
        Status::NotApplicable => "NOT APPLICABLE",
        Status::Uncertain => "UNCERTAIN",
        Status::Error => "ERROR",
        Status::Incomplete => "INCOMPLETE",
        Status::Skipped => "SKIPPED",
    }
}
/// The model's probability of a violation, as a note after a state.
pub(super) fn violation(probability: Option<f64>) -> String {
    probability
        .map(|p| format!(" · violation {:.0}%", p * 100.0))
        .unwrap_or_default()
}

/// The model's conclusion for a contract's input, its probability of a violation, and its raw
/// answer with the confidence threshold that routed it.
#[derive(Clone, Copy)]
pub(super) struct Model {
    pub status: ContractStatus,
    pub violation: Option<f64>,
    pub answer: Option<Answer>,
    pub min_confidence: Option<f64>,
}

/// Which way an unresolved answer leaned, and whether its confidence fell short of the threshold.
fn lean(model: Model) -> String {
    let Some(answer) = model.answer else {
        return String::new();
    };
    let below = match model.min_confidence {
        Some(threshold) if answer.confidence < threshold => format!(" < {threshold}"),
        _ => String::new(),
    };
    let choice = match answer.choice {
        Outcome::Match => "leans violation",
        Outcome::NoMatch => "leans no violation",
        Outcome::Uncertain => "answered uncertain",
    };
    format!(" · {choice} · confidence {:.2}{below}", answer.confidence)
}

/// A contract state's label and a note on what decided it: a record, and the model conclusion it
/// overrides; the model; or why the contract is pending. `detail` follows the model's answer.
pub(super) fn state(
    state: &ContractState,
    model: Option<Model>,
    detail: &str,
) -> (&'static str, String) {
    let violation = violation(model.and_then(|model| model.violation));
    let answer = |status: &str| format!("model {status}{violation}{detail}");
    let model_status = model.map_or("not assessed", |model| model.status.as_str());
    match state {
        ContractState::Settled {
            conclusion,
            record: Some(id),
        } => {
            let over = match model {
                Some(model) if model.status != *conclusion => {
                    format!(" over {}", answer(model_status))
                }
                _ => String::new(),
            };
            (upper(*conclusion), format!("recorded {id}{over}"))
        }
        ContractState::Settled {
            conclusion,
            record: None,
        } => (upper(*conclusion), format!("model{violation}{detail}")),
        ContractState::Pending { reason } => {
            let note = match reason {
                Pending::ModelFailed => format!("{} · confirm or dispute", answer("failed")),
                Pending::ModelUnresolved => {
                    format!(
                        "{}{}",
                        answer("unresolved"),
                        model.map(lean).unwrap_or_default()
                    )
                }
                Pending::NotAssessed => "not assessed".into(),
                Pending::InputsChanged => "inputs changed".into(),
                Pending::NotChecked => "not checked".into(),
                Pending::ContractChanged => {
                    format!("contract changed in this diff · {}", answer(model_status))
                }
            };
            ("PENDING", note)
        }
    }
}

fn upper(status: ContractStatus) -> &'static str {
    match status {
        ContractStatus::Verified => "VERIFIED",
        ContractStatus::Preserved => "PRESERVED",
        ContractStatus::Clear => "CLEAR",
        ContractStatus::Unaffected => "UNAFFECTED",
        ContractStatus::Failed => "FAILED",
        ContractStatus::Unresolved => "UNRESOLVED",
    }
}

/// A failed request's kind and whether a rerun may answer it.
pub(super) fn request_failure(failure: RequestFailure) -> String {
    let retry = if failure.retryable() {
        "retryable"
    } else {
        "not retryable"
    };
    format!("{}, {retry}", failure.kind().as_str())
}

/// The changed files no contract governs, when there are any.
pub(super) fn coverage(coverage: &Coverage) -> Option<String> {
    let files = |count: usize| format!("{count} changed file{}", if count == 1 { "" } else { "s" });
    if !coverage.governed && coverage.changed_files > 0 {
        return Some(format!(
            "No contract governs this change: {}",
            files(coverage.changed_files)
        ));
    }
    let ungoverned = &coverage.ungoverned_files;
    if ungoverned.is_empty() {
        return None;
    }
    let shown = ungoverned
        .iter()
        .take(5)
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let more = match ungoverned.len().saturating_sub(5) {
        0 => String::new(),
        more => format!(" and {more} more"),
    };
    Some(format!(
        "No contract governs {} of {}: {shown}{more}",
        ungoverned.len(),
        files(coverage.changed_files)
    ))
}

/// What a file check left out of its model input.
pub(super) fn omitted(omitted: &[Omitted]) -> String {
    if omitted.contains(&Omitted::Contents) {
        "judged from its patch only".into()
    } else if omitted.contains(&Omitted::Before) {
        "judged without prior contents".into()
    } else {
        String::new()
    }
}

/// The combined input's size and what was left out to fit the limit.
pub(super) fn context(context: &ContextSummary) -> String {
    let mut text = format!("{} of {MAX_FILE_BYTES} bytes", context.input_bytes);
    if context.input_bytes > MAX_FILE_BYTES {
        text.push_str(", too large; file checks only");
    }
    let mut omitted = Vec::new();
    if context.before_omitted {
        omitted.push("prior contents".to_owned());
    }
    if context.inventory_counted {
        omitted.push("inventory paths (counted)".to_owned());
    }
    if !context.contents_omitted.is_empty() {
        omitted.push(format!("{} file contents", context.contents_omitted.len()));
    }
    if !omitted.is_empty() {
        text.push_str(" · omitted ");
        text.push_str(&omitted.join(", "));
    }
    text
}

pub(super) fn counts(s: &Summary) -> String {
    let entries = [
        (s.passed, "passed"),
        (s.failed, "failed"),
        (s.uncertain, "uncertain"),
        (s.not_applicable, "not applicable"),
        (s.errors, "errors"),
        (s.incomplete, "incomplete"),
        (s.skipped, "skipped"),
    ];
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
        "PASS" | "VERIFIED" | "PRESERVED" | "CLEAR" => styled.green().to_string(),
        "FAIL" | "FAILED" | "ERROR" => styled.red().bold().to_string(),
        "UNCERTAIN" | "UNRESOLVED" | "INCOMPLETE" | "PENDING" => styled.yellow().to_string(),
        _ => styled.dim().to_string(),
    }
}

pub(super) fn marker(state: &str) -> String {
    let symbol = |symbol| console::style(symbol).for_stderr();
    match state {
        "PASS" | "VERIFIED" | "PRESERVED" | "CLEAR" => symbol("✓").green().to_string(),
        "FAIL" | "FAILED" | "ERROR" => symbol("×").red().to_string(),
        "UNCERTAIN" | "UNRESOLVED" | "INCOMPLETE" | "PENDING" => symbol("?").yellow().to_string(),
        _ => symbol("−").dim().to_string(),
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
