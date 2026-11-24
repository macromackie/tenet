use alloc::collections::BTreeSet;
use std::path::Path;

use super::{split_words, terms::OBSERVATION_PATTERNS};

const JSX_EXTENSIONS: &[&str] = &["jsx", "tsx"];
const SQL_BOUNDARY_CHARS: &[char] = &['\'', '"', '`', '('];
const SQL_DML_KEYWORDS: &[&str] = &[
    "create table ",
    "alter table ",
    "insert into ",
    "delete from ",
];
const SQL_PROSE_STOPWORDS: &[&str] = &[
    "", "a", "an", "its", "my", "our", "that", "the", "this", "your",
];
const SQL_TARGET_CHARS: &[char] = &['_', '.', '"', '`', ' '];

pub(super) struct Observed {
    pub signal_terms: BTreeSet<String>,
    pub signals: Vec<&'static str>,
    pub terms: BTreeSet<String>,
}

impl Observed {
    fn record(&mut self, label: &'static str, extra_terms: &[&str]) {
        self.signals.push(label);
        let words = split_words(label);
        self.terms.extend(words.iter().cloned());
        self.signal_terms.extend(words);
        for extra in extra_terms {
            let _inserted = self.terms.insert((*extra).to_owned());
            let _recorded = self.signal_terms.insert((*extra).to_owned());
        }
    }
}

pub(super) fn observe(path: &Path, source: &str) -> Observed {
    let mut observed = Observed {
        signal_terms: BTreeSet::new(),
        signals: Vec::new(),
        terms: BTreeSet::new(),
    };
    if source.lines().any(|line| {
        line.starts_with("export function ") || line.starts_with("export async function ")
    }) {
        observed.signals.push("an exported function");
        observed.terms.extend(split_words("an exported function"));
    }
    let code_lines: Vec<&str> = source
        .lines()
        .map(str::trim_start)
        .filter(|line| !(line.starts_with('#') || line.starts_with("//") || line.starts_with('*')))
        .collect();
    for (label, extra_terms, markers) in OBSERVATION_PATTERNS {
        if markers
            .iter()
            .any(|marker| code_lines.iter().any(|line| line.contains(marker)))
        {
            observed.record(label, extra_terms);
        }
    }
    if !observed.signals.contains(&"SQL database queries") && has_sql_statement(&code_lines) {
        observed.record(
            "SQL database queries",
            &["sql", "database", "store", "schema", "query"],
        );
    }
    if !observed.signal_terms.contains("jsx") && has_jsx_markup(path, &code_lines) {
        observed.record("JSX component markup", &["ui", "jsx", "component"]);
    }
    observed
}

pub(super) fn has_css_custom_property(source: &str) -> bool {
    source.lines().map(str::trim).any(|line| {
        !line.starts_with("/*")
            && !line.starts_with('*')
            && (line.starts_with(":root")
                || line.starts_with("@theme")
                || line.contains("var(--")
                || (line.starts_with("--") && line.contains(':')))
    })
}

fn has_jsx_markup(path: &Path, code_lines: &[&str]) -> bool {
    let is_jsx_ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| JSX_EXTENSIONS.contains(&ext));
    is_jsx_ext
        && code_lines
            .iter()
            .any(|line| line.contains("</") || line.contains("/>"))
}

fn has_sql_statement(code_lines: &[&str]) -> bool {
    code_lines.iter().any(|line| {
        let lower = line.to_ascii_lowercase();
        has_select_from(&lower) || has_sql_dml_keyword(&lower) || has_update_set(&lower)
    })
}

fn is_sql_boundary(prefix: &str) -> bool {
    prefix.chars().next_back().is_none_or(|character| {
        character.is_ascii_whitespace() || SQL_BOUNDARY_CHARS.contains(&character)
    })
}

fn has_select_from(lower: &str) -> bool {
    let Some((before, after)) = lower.split_once("select ") else {
        return false;
    };
    if !is_sql_boundary(before) {
        return false;
    }
    let Some((columns, table)) = after.split_once(" from ") else {
        return false;
    };
    let first_table = table.split_ascii_whitespace().next().unwrap_or_default();
    !columns.is_empty()
        && !columns.contains(['{', '}', ';'])
        && !SQL_PROSE_STOPWORDS.contains(&first_table)
}

fn has_sql_dml_keyword(lower: &str) -> bool {
    SQL_DML_KEYWORDS.iter().any(|keyword| {
        lower
            .split_once(keyword)
            .is_some_and(|(before, _after)| is_sql_boundary(before))
    })
}

fn has_update_set(lower: &str) -> bool {
    let Some((before, after)) = lower.split_once("update ") else {
        return false;
    };
    if !is_sql_boundary(before) {
        return false;
    }
    let Some((target, _assign)) = after.split_once(" set ") else {
        return false;
    };
    let trimmed = target.trim();
    !trimmed.is_empty()
        && trimmed.split_ascii_whitespace().count() <= 2
        && trimmed.chars().all(|character| {
            character.is_ascii_alphanumeric() || SQL_TARGET_CHARS.contains(&character)
        })
}
