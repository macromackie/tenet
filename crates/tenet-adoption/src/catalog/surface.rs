mod observe;
mod terms;

use alloc::collections::BTreeSet;
use std::{
    fs,
    io::Read as _,
    path::{Path, PathBuf},
};

use tenet_contracts::Contract;

use self::{
    observe::{has_css_custom_property, observe},
    terms::{COMMON_TERMS, DECLARATION_KEYWORDS, DOMAIN_PREFIXES, REQUIRED_FRAMEWORK_TERMS},
};
use super::{
    AdoptionError,
    profile::{RepositoryProfile, is_excluded_source_path},
    split_words,
};

pub(super) struct Surface {
    language: &'static str,
    path: PathBuf,
    signal_terms: BTreeSet<String>,
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
        if has_language_mismatch(self.language, &names)
            || has_missing_framework_term(&names, &self.terms)
            || !profile.is_domain_admissible(
                &contract.name,
                &names,
                &self.terms,
                &self.signal_terms,
            )
        {
            return None;
        }
        let title = count_title_matches(self.language, &names, &self.terms);
        if title == 0 {
            return None;
        }
        let prose = split_words(&contract.message);
        let shared = prose
            .intersection(&self.terms)
            .filter(|word| !COMMON_TERMS.contains(&word.as_str()))
            .count();
        let signal_title = count_title_matches(self.language, &names, &self.signal_terms);
        if is_domain_contract(&contract.name) && signal_title == 0 && (title < 2 || shared == 0) {
            return None;
        }
        let total = title.saturating_mul(4).saturating_add(shared);
        (total > 0).then_some(total)
    }
}

fn has_language_mismatch(language: &str, names: &BTreeSet<String>) -> bool {
    ["typescript", "python", "rust", "javascript", "go"]
        .iter()
        .any(|target| names.contains(*target) && !language.eq_ignore_ascii_case(target))
}

fn has_missing_framework_term(names: &BTreeSet<String>, terms: &BTreeSet<String>) -> bool {
    REQUIRED_FRAMEWORK_TERMS
        .iter()
        .any(|word| names.contains(*word) && !terms.contains(*word))
}

fn is_domain_contract(contract_name: &str) -> bool {
    contract_name
        .split_once('-')
        .is_some_and(|(prefix, _)| DOMAIN_PREFIXES.contains(&prefix))
}

fn count_title_matches(
    language: &str,
    names: &BTreeSet<String>,
    terms: &BTreeSet<String>,
) -> usize {
    names
        .intersection(terms)
        .filter(|word| {
            !language.eq_ignore_ascii_case(word) && !COMMON_TERMS.contains(&word.as_str())
        })
        .count()
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
        "css" | "scss" => Some("CSS"),
        "rs" => Some("Rust"),
        "go" => Some("Go"),
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
        "CSS" if has_css_custom_property(source) => {
            (
                "design tokens",
                "CSS design tokens",
                &["ui", "css", "design", "token", "theme"],
            )
        },
        "CSS" if source.contains('{') && source.contains(':') => {
            ("stylesheet", "CSS stylesheet rules", &["ui", "css"])
        },
        _ => return None,
    };
    let signal_terms: BTreeSet<String> =
        extra_terms.iter().map(|term| (*term).to_owned()).collect();
    let mut terms = split_words(&path.to_string_lossy());
    terms.extend(signal_terms.iter().cloned());
    Some(Surface {
        language,
        path: path.to_path_buf(),
        signal_terms,
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
    let observed = observe(path, source);
    let mut terms = split_words(&symbols.join(" "));
    terms.extend(observed.terms);
    terms.extend(split_words(&path.to_string_lossy()));
    let _inserted = terms.insert(language.to_ascii_lowercase());
    Some(Surface {
        language,
        path: path.to_path_buf(),
        signal_terms: observed.signal_terms,
        signals: observed.signals,
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
    !is_workflow_path(path) && is_excluded_source_path(path)
}
