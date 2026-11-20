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

fn detect_language(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()? {
        "ts" | "tsx" | "mts" | "cts" => Some("TypeScript"),
        "js" | "jsx" | "mjs" | "cjs" => Some("JavaScript"),
        "rs" => Some("Rust"),
        "py" => Some("Python"),
        _ => None,
    }
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
        let symbols: Vec<_> = source
            .lines()
            .filter_map(read_declaration)
            .take(8)
            .map(str::to_owned)
            .collect();
        if symbols.is_empty() {
            continue;
        }
        let (signals, signal_terms) = observe(&source);
        let mut terms = split_words(&symbols.join(" "));
        terms.extend(signal_terms);
        terms.extend(split_words(&path.to_string_lossy()));
        let _inserted = terms.insert(language.to_ascii_lowercase());
        found.push(Surface {
            language,
            path: path.clone(),
            signals,
            symbols,
            terms,
        });
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
        &["react"],
        &["from 'react'", r#"from "react""#],
    ),
    (
        "Effect programs",
        &["effect"],
        &["from 'effect'", r#"from "effect""#],
    ),
    (
        "Rust tracing spans",
        &["tracing"],
        &["tracing::", "use tracing"],
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
