use alloc::collections::BTreeMap;
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::anyhow;
use tenet_checks::behavior::AuditModuleInput;

use crate::error::CommandError;

const DOC_LIMIT: usize = 4;
const MAX_RUNTIME_PATHS: usize = 512;
const MAX_SHARD_BYTES: u64 = 100_000;
const MAX_SHARD_FILES: usize = 10;

/// Discovered module shards plus the repository runtime source closure for sandbox imports.
pub(super) struct DiscoveredAudit {
    pub modules: Vec<AuditModuleInput>,
    pub runtime_paths: Vec<String>,
}

const MANIFEST_NAMES: &[&str] = &[
    "Cargo.toml",
    "go.mod",
    "package.json",
    "pyproject.toml",
    "setup.cfg",
    "setup.py",
];

const README_NAMES: &[&str] = &["readme.md", "readme.rst", "readme.txt"];

const SKIPPED_SEGMENTS: &[&str] = &[
    ".contracts",
    ".github",
    "__pycache__",
    "__snapshots__",
    "__tests__",
    "bench",
    "benches",
    "build",
    "dist",
    "docs",
    "examples",
    "fixtures",
    "gen",
    "generated",
    "node_modules",
    "target",
    "test",
    "testdata",
    "tests",
    "third_party",
    "vendor",
];

const SOURCE_EXTENSIONS: &[&str] = &[
    "cjs", "cts", "go", "js", "jsx", "mjs", "mts", "py", "rs", "ts", "tsx",
];

const TEST_SUFFIXES: &[&str] = &[
    "_test.go",
    "_test.py",
    ".spec.cjs",
    ".spec.cts",
    ".spec.js",
    ".spec.mjs",
    ".spec.mts",
    ".spec.ts",
    ".test.cjs",
    ".test.cts",
    ".test.js",
    ".test.mjs",
    ".test.mts",
    ".test.ts",
];

/// Discover bounded production source modules under `root`, optionally filtered by `requested`.
pub(super) fn discover_modules(
    root: &Path,
    requested: &[PathBuf],
) -> Result<DiscoveredAudit, CommandError> {
    let filters = normalize_filters(requested)?;
    let all_paths = tenet_engine::list_paths(root)?;
    let manifests = collect_manifests(&all_paths);
    let docs = collect_docs(&all_paths);
    let mut runtime_paths = manifests.clone();
    let mut grouped_files: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in &all_paths {
        if !is_production_source(path) {
            continue;
        }
        let slash_path = format_slash_path(path);
        if runtime_paths.len() < MAX_RUNTIME_PATHS {
            runtime_paths.push(slash_path.clone());
        }
        if !is_matching_filter(path, &filters) {
            continue;
        }
        let dir_key = read_module_key(path);
        let entry = grouped_files.entry(dir_key).or_default();
        entry.push(slash_path);
    }
    let mut modules = Vec::new();
    for (base_name, mut files) in grouped_files {
        files.sort();
        let shards = split_into_shards(root, &base_name, files);
        for (name, shard_files) in shards {
            modules.push(AuditModuleInput {
                docs: docs.clone(),
                files: shard_files,
                manifests: manifests.clone(),
                name,
            });
        }
    }
    Ok(DiscoveredAudit {
        modules,
        runtime_paths,
    })
}

fn collect_docs(paths: &[PathBuf]) -> Vec<String> {
    let mut docs = Vec::new();
    for path in paths {
        let text = format_slash_path(path);
        let lower = text.to_ascii_lowercase();
        let is_readme = README_NAMES.contains(&lower.as_str());
        let is_doc_md = lower.starts_with("docs/")
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
        if is_readme || is_doc_md {
            docs.push(text);
        }
    }
    docs.sort();
    docs.truncate(DOC_LIMIT);
    docs
}

fn collect_manifests(paths: &[PathBuf]) -> Vec<String> {
    let mut manifests = Vec::new();
    for path in paths {
        if path
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty())
        {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|item| item.to_str())
            && MANIFEST_NAMES.contains(&name)
        {
            manifests.push(format_slash_path(path));
        }
    }
    manifests.sort();
    manifests
}

fn format_slash_path(path: &Path) -> String {
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

fn is_matching_filter(path: &Path, filters: &[PathBuf]) -> bool {
    if filters.is_empty() {
        return true;
    }
    filters
        .iter()
        .any(|filter| filter.as_os_str().is_empty() || path == filter || path.starts_with(filter))
}

fn is_production_source(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|item| item.to_str()) else {
        return false;
    };
    if !SOURCE_EXTENSIONS.contains(&ext) {
        return false;
    }
    for component in path.components() {
        if let Component::Normal(segment) = component {
            let text = segment.to_string_lossy();
            if SKIPPED_SEGMENTS.contains(&text.as_ref()) {
                return false;
            }
        }
    }
    let Some(name) = path.file_name().and_then(|item| item.to_str()) else {
        return false;
    };
    if name.starts_with("test_") && ext == "py" {
        return false;
    }
    !TEST_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

fn normalize_filters(requested: &[PathBuf]) -> Result<Vec<PathBuf>, CommandError> {
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

fn read_module_key(path: &Path) -> String {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => format_slash_path(parent),
        _ => String::from("root"),
    }
}

fn split_into_shards(
    root: &Path,
    base_name: &str,
    files: Vec<String>,
) -> Vec<(String, Vec<String>)> {
    let mut buckets: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_bytes: u64 = 0;
    for file in files {
        let size = fs::metadata(root.join(&file)).map_or(0, |meta| meta.len());
        let exceeds_files = current.len() >= MAX_SHARD_FILES;
        let exceeds_bytes =
            !current.is_empty() && current_bytes.saturating_add(size) > MAX_SHARD_BYTES;
        if exceeds_files || exceeds_bytes {
            buckets.push(current);
            current = Vec::new();
            current_bytes = 0;
        }
        current.push(file);
        current_bytes = current_bytes.saturating_add(size);
    }
    if !current.is_empty() {
        buckets.push(current);
    }
    if buckets.len() <= 1 {
        return buckets
            .into_iter()
            .map(|bucket| (base_name.to_owned(), bucket))
            .collect();
    }
    buckets
        .into_iter()
        .enumerate()
        .map(|(index, bucket)| {
            let number = index.saturating_add(1);
            (format!("{base_name}#{number}"), bucket)
        })
        .collect()
}
