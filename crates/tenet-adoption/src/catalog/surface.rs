use alloc::collections::BTreeSet;
use std::{
    fs,
    io::Read as _,
    path::{Path, PathBuf},
};

use tenet_contracts::Contract;

use super::{AdoptionError, profile::RepositoryProfile, split_words};

const COMMON_TERMS: &[&str] = &[
    "arg", "base", "build", "call", "check", "class", "client", "close", "code", "config",
    "context", "core", "count", "create", "data", "default", "entry", "error", "field", "file",
    "flag", "form", "format", "function", "helper", "index", "init", "input", "item", "key", "lib",
    "line", "list", "main", "map", "member", "method", "mode", "module", "name", "note", "object",
    "open", "option", "order", "output", "package", "param", "path", "read", "record", "result",
    "role", "rule", "run", "scope", "set", "shape", "size", "source", "src", "state", "string",
    "style", "test", "text", "type", "util", "value", "write",
];

const DECLARATION_KEYWORDS: &[&str] = &[
    "async",
    "class",
    "const",
    "def",
    "default",
    "enum",
    "fn",
    "function",
    "interface",
    "let",
    "mod",
    "struct",
    "trait",
    "type",
    "var",
];

const REQUIRED_FRAMEWORK_TERMS: &[&str] = &[
    "alembic",
    "celld",
    "clippy",
    "effect",
    "fastapi",
    "oauth",
    "openapi",
    "oxlint",
    "postgres",
    "react",
    "ruff",
    "sql",
    "storybook",
    "webhook",
];

pub(super) struct Surface {
    language: &'static str,
    path: PathBuf,
    signals: Vec<&'static str>,
    symbols: Vec<String>,
    terms: BTreeSet<String>,
}

impl Surface {
    pub(super) fn explain_match(&self) -> String {
        let declared = format!("{} declares {}.", self.language, self.symbols.join(", "));
        if self.signals.is_empty() {
            declared
        } else {
            format!("{declared} Observed {}.", self.signals.join(", "))
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn score(&self, contract: &Contract, profile: &RepositoryProfile) -> Option<usize> {
        let names = split_words(&contract.name);
        if ["typescript", "python", "rust", "javascript"]
            .iter()
            .any(|language| {
                names.contains(*language) && !self.language.eq_ignore_ascii_case(language)
            })
        {
            return None;
        }
        if REQUIRED_FRAMEWORK_TERMS
            .iter()
            .any(|word| names.contains(*word) && !self.terms.contains(*word))
        {
            return None;
        }
        if !profile.is_domain_admissible(&contract.name, &names, &self.terms) {
            return None;
        }
        let title = names
            .intersection(&self.terms)
            .filter(|word| {
                !self.language.eq_ignore_ascii_case(word) && !COMMON_TERMS.contains(&word.as_str())
            })
            .count();
        if title == 0 {
            return None;
        }
        let prose = split_words(&contract.message);
        let shared = prose
            .intersection(&self.terms)
            .filter(|word| !COMMON_TERMS.contains(&word.as_str()))
            .count();
        let total = title.saturating_mul(4).saturating_add(shared);
        (total > 0).then_some(total)
    }
}

fn is_workflow_path(path: &Path) -> bool {
    path.starts_with(".github/workflows")
        && matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("yml" | "yaml")
        )
}

fn detect_language(path: &Path) -> Option<&'static str> {
    if is_workflow_path(path) {
        return Some("GitHub Actions");
    }
    match path.extension()?.to_str()? {
        "ts" | "tsx" | "mts" | "cts" => Some("TypeScript"),
        "js" | "jsx" | "mjs" | "cjs" => Some("JavaScript"),
        "rs" => Some("Rust"),
        "py" => Some("Python"),
        "tf" => Some("Terraform"),
        _ => None,
    }
}

fn build_declarative_surface(path: &Path, language: &'static str, source: &str) -> Option<Surface> {
    let (symbol, signal, extra_terms): (&str, &'static str, &[&str]) = match language {
        "GitHub Actions" if source.contains("jobs:") || source.contains("runs-on:") => {
            (
                "workflow",
                "CI workflow checks",
                &["ci", "check", "trust", "reproducible", "workflow"],
            )
        },
        "Terraform"
            if source.contains("resource ")
                || source.contains("module ")
                || source.contains("terraform ")
                || source.contains("provider ") =>
        {
            (
                "terraform",
                "Terraform infrastructure definitions",
                &["infra", "terraform", "state", "apply", "authority"],
            )
        },
        _ => return None,
    };
    let mut terms = split_words(&path.to_string_lossy());
    terms.extend(extra_terms.iter().map(|term| (*term).to_owned()));
    Some(Surface {
        language,
        path: path.to_path_buf(),
        signals: vec![signal],
        symbols: vec![symbol.to_owned()],
        terms,
    })
}

fn read_declaration(line: &str) -> Option<&str> {
    let trimmed = line.strip_prefix("    ").unwrap_or(line);
    let prefix = ["export ", "pub ", "async def ", "def ", "class "]
        .into_iter()
        .find(|prefix| trimmed.starts_with(prefix))?;
    let mut words = trimmed
        .strip_prefix(prefix)?
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter(|name| !name.is_empty());
    let name = words.find(|name| !DECLARATION_KEYWORDS.contains(name))?;
    (name.len() <= 80
        && name.starts_with(|first: char| first.is_ascii_alphabetic())
        && !name.starts_with('_'))
    .then_some(name)
}

fn build_code_surface(path: &Path, language: &'static str, source: &str) -> Option<Surface> {
    let symbols: Vec<_> = source
        .lines()
        .filter_map(read_declaration)
        .take(8)
        .map(str::to_owned)
        .collect();
    if symbols.is_empty() {
        return None;
    }
    let (signals, signal_terms) = observe(source);
    let mut terms = split_words(&symbols.join(" "));
    terms.extend(signal_terms);
    terms.extend(split_words(&path.to_string_lossy()));
    let _inserted = terms.insert(language.to_ascii_lowercase());
    Some(Surface {
        language,
        path: path.to_path_buf(),
        signals,
        symbols,
        terms,
    })
}

pub(super) fn inspect(root: &Path, inventory: &[PathBuf]) -> Result<Vec<Surface>, AdoptionError> {
    let mut found = Vec::new();
    let mut scanned = 0_usize;
    for path in inventory {
        let Some(language) = detect_language(path) else {
            continue;
        };
        if is_support_path(path) {
            continue;
        }
        if scanned >= 256 {
            break;
        }
        let absolute = root.join(path);
        let metadata = absolute
            .symlink_metadata()
            .map_err(AdoptionError::Scratch)?;
        if !metadata.is_file() {
            continue;
        }
        scanned = scanned.saturating_add(1);
        let mut bytes = Vec::new();
        let _size = fs::File::open(&absolute)
            .and_then(|file| file.take(0x0001_0000).read_to_end(&mut bytes))
            .map_err(AdoptionError::Scratch)?;
        let source = String::from_utf8_lossy(&bytes);
        if let Some(surface) = build_declarative_surface(path, language, &source)
            .or_else(|| build_code_surface(path, language, &source))
        {
            found.push(surface);
        }
    }
    Ok(found)
}

fn is_support_path(path: &Path) -> bool {
    const SUPPORT: &[&str] = &[
        "dist",
        "examples",
        "fixtures",
        "node_modules",
        "scripts",
        "target",
        "test",
        "tests",
        "tools",
        "vendor",
    ];
    if is_workflow_path(path) {
        return false;
    }
    path.iter().any(|part| {
        part.to_str()
            .is_none_or(|name| name.starts_with('.') || SUPPORT.contains(&name))
    })
}

const OBSERVATION_PATTERNS: &[(&str, &[&str], &[&str])] = &[
    (
        "explicit input precedence with a default fallback",
        &[],
        &[
            "??",
            ".unwrap_or(",
            ".unwrap_or_else(",
            " is None",
            " is not None",
        ],
    ),
    (
        "borrowed inputs modified in place",
        &[],
        &[
            ".sort(",
            ".splice(",
            ".append(",
            ".extend(",
            ".update(",
            ".sort_unstable(",
        ],
    ),
    (
        "equivalent representations across normalized inputs",
        &[],
        &["os.fspath", "PathLike", ".casefold(", "FromStr"],
    ),
    (
        "idempotent source transformations",
        &["idempotent", "transformation"],
        &[
            "TokenStream",
            "tokenize.",
            "import tokenize",
            "ast.parse(",
            "format_code(",
            "sort_code(",
            "sort_imports(",
            "pretty_print(",
        ],
    ),
    (
        "broad Python exception propagation handler",
        &[],
        &["except Exception", "except:"],
    ),
    (
        "FastAPI routes",
        &["fastapi", "api", "route"],
        &["from fastapi import", "import fastapi"],
    ),
    (
        "React components",
        &["react", "component", "jsx"],
        &["from 'react'", r#"from "react""#],
    ),
    (
        "React hook calls",
        &["react", "hook"],
        &[
            "useEffect(",
            "useState(",
            "useMemo(",
            "useCallback(",
            "useRef(",
        ],
    ),
    (
        "Effect programs",
        &["effect", "schema", "error"],
        &["from 'effect'", r#"from "effect""#],
    ),
    (
        "Rust tracing spans",
        &["tracing", "telemetry", "span"],
        &["tracing::", "use tracing"],
    ),
    (
        "CLI output channels",
        &["cli", "output", "channel"],
        &[
            "process.stdout.write(",
            "process.stderr.write(",
            "io::stdout(",
            "io::stderr(",
            "sys.stdout",
            "sys.stderr",
        ],
    ),
    (
        "SQL database queries",
        &["sql", "database", "store", "schema"],
        &[
            "conn.execute(",
            "cursor.execute(",
            "sqlx::",
            "from sqlalchemy",
            "import sqlalchemy",
        ],
    ),
];

fn observe(source: &str) -> (Vec<&'static str>, BTreeSet<String>) {
    let mut signals = Vec::new();
    let mut terms = BTreeSet::new();
    if source.lines().any(|line| {
        line.starts_with("export function ") || line.starts_with("export async function ")
    }) {
        signals.push("an exported function");
        terms.extend(split_words("an exported function"));
    }
    for (label, extra_terms, markers) in OBSERVATION_PATTERNS {
        if markers.iter().any(|marker| {
            source
                .lines()
                .map(str::trim_start)
                .filter(|line| {
                    !(line.starts_with('#') || line.starts_with("//") || line.starts_with('*'))
                })
                .any(|line| line.contains(marker))
        }) {
            signals.push(label);
            terms.extend(split_words(label));
            terms.extend(extra_terms.iter().map(|term| (*term).to_owned()));
        }
    }
    (signals, terms)
}
