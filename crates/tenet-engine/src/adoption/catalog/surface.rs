use alloc::collections::BTreeSet;
use std::{
    fs,
    io::Read as _,
    path::{Path, PathBuf},
};

use tenet_contracts::Contract;

use super::{AdoptionError, split_words};

const COMMON_TERMS: &[&str] = &["src", "code", "client", "lib"];

const DECLARATION_KEYWORDS: &[&str] = &[
    "default",
    "async",
    "const",
    "let",
    "var",
    "class",
    "function",
    "fn",
    "struct",
    "enum",
    "interface",
    "type",
    "trait",
    "mod",
    "def",
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

    pub(super) fn score(&self, contract: &Contract) -> Option<usize> {
        let names = split_words(&contract.name);
        if ["typescript", "python", "rust", "javascript"]
            .iter()
            .any(|language| {
                names.contains(*language) && !self.language.eq_ignore_ascii_case(language)
            })
        {
            return None;
        }
        if [
            "react", "fastapi", "alembic", "postgres", "celld", "oxlint", "ruff", "clippy",
        ]
        .iter()
        .any(|word| names.contains(*word) && !self.terms.contains(*word))
        {
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
        let shared = prose.intersection(&self.terms).count();
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
    let prefix = ["export ", "pub ", "async def ", "def ", "class "]
        .into_iter()
        .find(|prefix| line.starts_with(prefix))?;
    let mut words = line
        .strip_prefix(prefix)?
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter(|name| !name.is_empty());
    let name = words.find(|name| !DECLARATION_KEYWORDS.contains(name))?;
    (name.len() <= 80 && name.starts_with(|first: char| first.is_ascii_alphabetic()))
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
            .and_then(|file| file.take(0x4000).read_to_end(&mut bytes))
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
        let signals = observe(&source);
        let mut terms = split_words(&symbols.join(" "));
        terms.extend(split_words(&signals.join(" ")));
        terms.extend(split_words(&path.to_string_lossy()));
        terms.extend(split_words(language));
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
        "scripts",
        "tools",
        "tests",
        "fixtures",
        "examples",
        "node_modules",
        "vendor",
        "target",
        "dist",
    ];
    path.iter().any(|part| {
        part.to_str()
            .is_none_or(|name| name.starts_with('.') || SUPPORT.contains(&name))
    })
}

fn observe(source: &str) -> Vec<&'static str> {
    let mut signals = Vec::new();
    if source.lines().any(|line| {
        line.starts_with("export function ") || line.starts_with("export async function ")
    }) {
        signals.push("an exported function");
    }
    for (label, markers) in [
        (
            "an explicit input with a default fallback",
            &["??", ".unwrap_or(", ".unwrap_or_else(", " is None"][..],
        ),
        (
            "in-place collection mutation",
            &[".sort(", ".splice(", ".append(", ".sort_unstable("][..],
        ),
        (
            "FastAPI routes",
            &["from fastapi import", "import fastapi"][..],
        ),
        ("React components", &["from 'react'", r#"from "react""#][..]),
    ] {
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
        }
    }
    signals
}
