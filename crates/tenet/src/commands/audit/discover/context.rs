use alloc::collections::BTreeSet;
use std::{
    fs::File,
    io::Read as _,
    path::{Component, Path, PathBuf},
};

const DOC_LIMIT: usize = 4;
const GENERIC_STEMS: &[&str] = &["__init__", "index", "init", "lib", "main", "mod"];
const LOW_TEST_THRESHOLD: usize = 1;
const MAX_TEST_FILE_BYTES: usize = 16_000;
const MAX_TEST_FILES: usize = 192;
const MAX_TEST_TOTAL_BYTES: usize = 1_000_000;
const PATH_MATCH_WEIGHT: usize = 3;
const README_NAMES: &[&str] = &["readme.md", "readme.rst", "readme.txt"];
const TEST_LIMIT: usize = 4;

struct TestEntry {
    lower_path: String,
    path: String,
    text: String,
}

/// Compact lowercase snapshot of repository test files used to spot under-tested source files.
pub(super) struct TestIndex {
    entries: Vec<TestEntry>,
}

impl TestIndex {
    /// Scan up to 192 test files (bounded to 1 MB total) to measure test coverage mentions.
    pub(super) fn build(root: &Path, test_paths: &[PathBuf]) -> Self {
        let mut entries = Vec::new();
        let mut remaining = MAX_TEST_TOTAL_BYTES;
        for path in test_paths.iter().take(MAX_TEST_FILES) {
            if remaining == 0 {
                break;
            }
            let limit = remaining.min(MAX_TEST_FILE_BYTES);
            let slash_path = format_slash_path(path);
            let lower_path = slash_path.to_ascii_lowercase();
            let mut text = lower_path.clone();
            let snippet = read_bounded_lowercase(&root.join(path), limit);
            remaining = remaining.saturating_sub(snippet.len());
            text.push('\n');
            text.push_str(&snippet);
            entries.push(TestEntry {
                lower_path,
                path: slash_path,
                text,
            });
        }
        Self { entries }
    }

    /// Return production files in `files` mentioned in at most one test file, least-tested first.
    pub(super) fn find_low_test_files(&self, files: &[String]) -> Vec<String> {
        if self.entries.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(usize, String)> = files
            .iter()
            .filter_map(|file| {
                let token = read_subject_token(file)?;
                let hits = self
                    .entries
                    .iter()
                    .filter(|entry| entry.text.contains(token.as_str()))
                    .count();
                (hits <= LOW_TEST_THRESHOLD).then(|| (hits, file.clone()))
            })
            .collect();
        scored.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        scored.into_iter().map(|(_, file)| file).collect()
    }

    /// Return up to four existing test file paths most relevant to `module_name` and `files`.
    pub(super) fn find_related_test_files(
        &self,
        module_name: &str,
        files: &[String],
    ) -> Vec<String> {
        if self.entries.is_empty() {
            return Vec::new();
        }
        let keywords = collect_keywords(module_name, files);
        if keywords.is_empty() {
            return Vec::new();
        }
        let ranked: Vec<(usize, String)> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let path_hits = keywords
                    .iter()
                    .filter(|token| entry.lower_path.contains(token.as_str()))
                    .count()
                    .saturating_mul(PATH_MATCH_WEIGHT);
                let text_hits = keywords
                    .iter()
                    .filter(|token| entry.text.contains(token.as_str()))
                    .count();
                let score = path_hits.saturating_add(text_hits);
                (score > 0).then(|| (score, entry.path.clone()))
            })
            .collect();
        take_top_ranked(ranked, TEST_LIMIT)
    }
}

/// Collect root READMEs and `docs/` Markdown or reStructuredText files across the repository.
pub(super) fn collect_all_docs(paths: &[PathBuf]) -> Vec<String> {
    let mut docs = Vec::new();
    for path in paths {
        let text = format_slash_path(path);
        let lower = text.to_ascii_lowercase();
        let is_readme = README_NAMES.contains(&lower.as_str());
        let is_doc = lower.starts_with("docs/")
            && path.extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("rst")
            });
        if is_readme || is_doc {
            docs.push(text);
        }
    }
    docs.sort();
    docs
}

/// Format a relative repository path using forward slashes across platforms.
pub(super) fn format_slash_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| {
            match component {
                Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
                Component::CurDir
                | Component::ParentDir
                | Component::Prefix(_)
                | Component::RootDir => None,
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Read up to `limit` bytes from `path` and return them lowercased, or empty on I/O errors.
pub(super) fn read_bounded_lowercase(path: &Path, limit: usize) -> String {
    let Ok(file) = File::open(path) else {
        return String::new();
    };
    let mut bytes = Vec::new();
    let Ok(limit_u64) = u64::try_from(limit) else {
        return String::new();
    };
    if file.take(limit_u64).read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&bytes).to_ascii_lowercase()
}

/// Extract a non-generic lowercase subject stem from `file` (falling back to its parent folder).
pub(super) fn read_subject_token(file: &str) -> Option<String> {
    let path = Path::new(file);
    let stem = path
        .file_stem()
        .and_then(|item| item.to_str())
        .map(str::to_ascii_lowercase)?;
    if stem.len() >= 3 && !GENERIC_STEMS.contains(&stem.as_str()) {
        return Some(stem);
    }
    let parent = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|item| item.to_str())
        .map(str::to_ascii_lowercase)?;
    if parent.len() >= 3 && !GENERIC_STEMS.contains(&parent.as_str()) {
        return Some(parent);
    }
    None
}

/// Rank repository docs for one module, keeping root READMEs and highest keyword overlap first.
pub(super) fn select_module_docs(
    all_docs: &[String],
    module_name: &str,
    files: &[String],
) -> Vec<String> {
    let keywords = collect_keywords(module_name, files);
    let ranked: Vec<(usize, String)> = all_docs
        .iter()
        .map(|doc| {
            let lower = doc.to_ascii_lowercase();
            let readme_bonus =
                usize::from(README_NAMES.contains(&lower.as_str())).saturating_mul(100);
            let overlap = keywords
                .iter()
                .filter(|token| lower.contains(token.as_str()))
                .count();
            (readme_bonus.saturating_add(overlap), doc.clone())
        })
        .collect();
    take_top_ranked(ranked, DOC_LIMIT)
}

/// Sort scored strings descending by score (then lexicographically) and keep at most `limit`.
pub(super) fn take_top_ranked(mut ranked: Vec<(usize, String)>, limit: usize) -> Vec<String> {
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    ranked
        .into_iter()
        .take(limit)
        .map(|(_, item)| item)
        .collect()
}

fn collect_keywords(module_name: &str, files: &[String]) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();
    for part in module_name.split(['/', '#', '_', '-']) {
        let clean = part.to_ascii_lowercase();
        if clean.len() >= 3 && !GENERIC_STEMS.contains(&clean.as_str()) {
            let _inserted = tokens.insert(clean);
        }
    }
    for file in files {
        if let Some(token) = read_subject_token(file) {
            let _inserted = tokens.insert(token);
        }
    }
    tokens
}
