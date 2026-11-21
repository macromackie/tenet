mod shard;

use alloc::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use anyhow::anyhow;
use shard::{merge_tiny_siblings, split_into_shards};
use tenet_checks::behavior::AuditModuleInput;

use crate::error::CommandError;

const AUDIT_ONLY_SKIP_SEGMENTS: &[&str] = &[
    "gen",
    "generated",
    "i18n",
    "l10n",
    "locale",
    "locales",
    "translations",
];
const DOC_LIMIT: usize = 4;
const MANIFEST_NAMES: &[&str] = &[
    "Cargo.toml",
    "go.mod",
    "package.json",
    "pyproject.toml",
    "setup.cfg",
    "setup.py",
];
const MAX_RUNTIME_PATHS: usize = 1_024;
const NON_RUNTIME_SEGMENTS: &[&str] = &[
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
    "node_modules",
    "target",
    "test",
    "testdata",
    "tests",
    "third_party",
    "vendor",
];
const README_NAMES: &[&str] = &["readme.md", "readme.rst", "readme.txt"];
const SOURCE_EXTENSIONS: &[&str] = &[
    "cjs", "cts", "go", "js", "jsx", "mjs", "mts", "py", "rs", "ts", "tsx",
];

/// Discovered module shards plus the repository runtime source closure for sandbox imports.
pub(super) struct DiscoveredAudit {
    pub modules: Vec<AuditModuleInput>,
    pub runtime_paths: Vec<String>,
}

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
    let mut module_files: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in &all_paths {
        if !is_runtime_source(path) {
            continue;
        }
        let slash_path = format_slash_path(path);
        if runtime_paths.len() < MAX_RUNTIME_PATHS {
            runtime_paths.push(slash_path.clone());
        }
        if !is_auditable_source(path) || !is_matching_filter(path, &filters) {
            continue;
        }
        let dir_key = read_module_key(path);
        let entry = module_files.entry(dir_key).or_default();
        entry.push(slash_path);
    }
    let merged_modules = merge_tiny_siblings(module_files);
    let mut modules = Vec::new();
    for (base_name, mut files) in merged_modules {
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

fn has_segment(path: &Path, blocked: &[&str]) -> bool {
    path.components().any(|component| {
        let Component::Normal(segment) = component else {
            return false;
        };
        blocked.contains(&segment.to_string_lossy().as_ref())
    })
}

fn is_auditable_source(path: &Path) -> bool {
    !has_segment(path, AUDIT_ONLY_SKIP_SEGMENTS)
}

fn is_matching_filter(path: &Path, filters: &[PathBuf]) -> bool {
    filters.is_empty()
        || filters.iter().any(|filter| {
            filter.as_os_str().is_empty() || path == filter || path.starts_with(filter)
        })
}

fn is_runtime_source(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|item| item.to_str()) else {
        return false;
    };
    if !SOURCE_EXTENSIONS.contains(&ext) || has_segment(path, NON_RUNTIME_SEGMENTS) {
        return false;
    }
    let Some(name) = path.file_name().and_then(|item| item.to_str()) else {
        return false;
    };
    !is_test_filename(name, ext)
}

fn is_test_filename(name: &str, ext: &str) -> bool {
    let is_python_or_go_test = (ext == "py" && name.starts_with("test_"))
        || name.ends_with("_test.go")
        || name.ends_with("_test.py");
    let stem = name
        .strip_suffix(ext)
        .and_then(|prefix| prefix.strip_suffix('.'));
    is_python_or_go_test
        || stem.is_some_and(|base| {
            Path::new(base).extension().is_some_and(|tag| {
                tag.eq_ignore_ascii_case("test") || tag.eq_ignore_ascii_case("spec")
            })
        })
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
