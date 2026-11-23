use alloc::collections::BTreeSet;
use core::str;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context as _, anyhow};
use tenet_engine::run_checkout_git;

use super::context::{
    format_slash_path,
    read_bounded_lowercase,
    read_subject_token,
    take_top_ranked,
};
use crate::error::CommandError;

const CODE_EXTENSIONS: &[&str] = &[
    "cjs", "cts", "go", "js", "jsx", "mjs", "mts", "py", "rs", "ts", "tsx",
];
const DIRECT_IMPORT_WEIGHT: usize = 4;
const LOGIC_MARKERS: &[&str] = &[
    "class ",
    "def ",
    "export ",
    "fn ",
    "func ",
    "function ",
    "impl ",
    "=>",
];
const MAX_HEADER_BYTES: usize = 8_000;
const MAX_INDEX_FILES: usize = 256;
const MAX_LOGIC_BYTES: usize = 16_000;
const MAX_SCAN_FILES: usize = 16;
const MIN_STEM_LEN: usize = 4;
const RELATED_FILE_LIMIT: usize = 4;
const SAME_DIR_GO_WEIGHT: usize = 2;
const STEM_MENTION_WEIGHT: usize = 2;

/// Pre-read lowercase source headers across runtime files for O(N) fan-in centrality scoring.
pub(super) struct HeaderIndex {
    entries: Vec<(String, String)>,
}

impl HeaderIndex {
    /// Read bounded lowercase headers for up to `MAX_INDEX_FILES` runtime code files.
    pub(super) fn build(root: &Path, runtime_paths: &[String]) -> Self {
        let entries = runtime_paths
            .iter()
            .filter(|path| is_code_file(path))
            .take(MAX_INDEX_FILES)
            .filter_map(|path| {
                let text = read_bounded_lowercase(&root.join(path), MAX_HEADER_BYTES);
                (!text.is_empty()).then(|| (path.clone(), text))
            })
            .collect();
        Self { entries }
    }

    /// Count external runtime files that reference or import any file in `shard_files`.
    pub(super) fn count_inbound_refs(&self, shard_files: &[String]) -> usize {
        let included_paths: BTreeSet<&str> = shard_files.iter().map(String::as_str).collect();
        let stems: Vec<String> = shard_files
            .iter()
            .filter_map(|path| read_subject_token(path))
            .filter(|stem| stem.len() >= MIN_STEM_LEN)
            .collect();
        if stems.is_empty() {
            return 0;
        }
        self.entries
            .iter()
            .filter(|(path, text)| {
                !included_paths.contains(path.as_str())
                    && stems.iter().any(|stem| has_direct_import(text, stem))
            })
            .count()
    }
}

/// Return true when at least one file in `shard_files` defines functions, classes, or exports.
pub(super) fn has_executable_logic(root: &Path, shard_files: &[String]) -> bool {
    shard_files.iter().take(MAX_SCAN_FILES).any(|path| {
        let snippet = read_bounded_lowercase(&root.join(path), MAX_LOGIC_BYTES);
        LOGIC_MARKERS.iter().any(|marker| snippet.contains(marker))
    })
}

/// Find up to four external production files referenced by or sharing a Go package with
/// `shard_files`.
pub(in crate::commands) fn find_related_files(
    root: &Path,
    shard_files: &[String],
    runtime_paths: &[String],
) -> Vec<String> {
    let included_paths: BTreeSet<&str> = shard_files.iter().map(String::as_str).collect();
    let shard_dirs: BTreeSet<&str> = shard_files
        .iter()
        .map(|path| read_parent_dir(path))
        .collect();
    let headers = read_shard_headers(root, shard_files);
    let ranked: Vec<(usize, String)> = runtime_paths
        .iter()
        .filter(|path| !included_paths.contains(path.as_str()) && is_code_file(path))
        .filter_map(|candidate| {
            let score = score_candidate(candidate, &headers, &shard_dirs);
            (score > 0).then(|| (score, candidate.clone()))
        })
        .collect();
    take_top_ranked(ranked, RELATED_FILE_LIMIT)
}

/// Filter `shard_files` to paths present in `changed_paths`.
pub(super) fn collect_shard_changes(
    shard_files: &[String],
    changed_paths: Option<&BTreeSet<String>>,
) -> Vec<String> {
    let Some(changed_set) = changed_paths else {
        return Vec::new();
    };
    shard_files
        .iter()
        .filter(|file| changed_set.contains(file.as_str()))
        .cloned()
        .collect()
}

/// Normalize user-provided filter paths and reject paths escaping the repository root.
pub(in crate::commands) fn normalize_filters(
    requested: &[PathBuf],
) -> Result<Vec<PathBuf>, CommandError> {
    requested
        .iter()
        .map(|raw| {
            let mut normalized = PathBuf::new();
            for component in raw.components() {
                if let Component::Normal(segment) = component {
                    normalized.push(segment);
                    continue;
                }
                if !matches!(component, Component::CurDir) {
                    return Err(anyhow!(
                        "audit paths must stay inside the repository root: {:?}",
                        raw.display()
                    )
                    .into());
                }
            }
            Ok(normalized)
        })
        .collect()
}

/// Resolve tracked diff paths against `base_ref` plus untracked non-ignored paths in `root`.
pub(super) fn list_changed_files(
    root: &Path,
    base_ref: &str,
) -> Result<BTreeSet<String>, CommandError> {
    if base_ref.starts_with('-') {
        return Err(anyhow!("Git reference cannot start with '-': {base_ref:?}").into());
    }
    let diff_bytes = run_checkout_git(root, &["diff", "--name-only", "-z", base_ref, "--"])
        .with_context(|| format!("list changed files against {base_ref:?}"))?;
    let untracked_bytes =
        run_checkout_git(root, &["ls-files", "--others", "--exclude-standard", "-z"])
            .context("list untracked files for audit diff")?;
    let mut changed_paths = BTreeSet::new();
    collect_nul_paths(&diff_bytes, &mut changed_paths);
    collect_nul_paths(&untracked_bytes, &mut changed_paths);
    Ok(changed_paths)
}

fn collect_nul_paths(bytes: &[u8], changed_paths: &mut BTreeSet<String>) {
    for slice in bytes.split(|byte| *byte == b'\0') {
        if slice.is_empty() {
            continue;
        }
        if let Ok(raw) = str::from_utf8(slice) {
            let clean = format_slash_path(Path::new(raw));
            if !clean.is_empty() {
                let _inserted = changed_paths.insert(clean);
            }
        }
    }
}

fn has_direct_import(headers: &str, stem: &str) -> bool {
    let patterns = [
        format!("/{stem}"),
        format!(".{stem}"),
        format!("::{stem}"),
        format!("\"{stem}\""),
        format!("'{stem}'"),
    ];
    patterns
        .iter()
        .any(|needle| headers.contains(needle.as_str()))
}

fn is_code_file(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| CODE_EXTENSIONS.contains(&ext))
}

fn read_parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn read_shard_headers(root: &Path, shard_files: &[String]) -> String {
    let mut combined = String::new();
    for path in shard_files.iter().take(MAX_SCAN_FILES) {
        let snippet = read_bounded_lowercase(&root.join(path), MAX_HEADER_BYTES);
        if !snippet.is_empty() {
            combined.push_str(&snippet);
            combined.push('\n');
        }
    }
    combined
}

fn score_candidate(candidate: &str, headers: &str, shard_dirs: &BTreeSet<&str>) -> usize {
    let mut score = 0_usize;
    if Path::new(candidate)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("go"))
        && shard_dirs.contains(read_parent_dir(candidate))
    {
        score = score.saturating_add(SAME_DIR_GO_WEIGHT);
    }
    let Some(stem) = read_subject_token(candidate) else {
        return score;
    };
    if has_direct_import(headers, &stem) {
        return score.saturating_add(DIRECT_IMPORT_WEIGHT);
    }
    if stem.len() >= MIN_STEM_LEN && headers.contains(stem.as_str()) {
        return score.saturating_add(STEM_MENTION_WEIGHT);
    }
    score
}
