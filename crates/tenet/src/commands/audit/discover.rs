mod context;
mod seams;
mod shard;

use alloc::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use context::{
    TestIndex,
    collect_all_docs,
    collect_manifests,
    format_slash_path,
    select_module_docs,
};
use seams::{
    HeaderIndex,
    collect_shard_changes,
    find_related_files,
    has_executable_logic,
    list_changed_files,
    normalize_filters,
};
use shard::{merge_tiny_siblings, split_into_shards};
use tenet_checks::behavior::AuditModuleInput;

use crate::error::CommandError;

const AUDIT_ONLY_SKIP_SEGMENTS: &[&str] = &[
    "_vendor",
    "_vendored",
    "codegen",
    "gen",
    "generated",
    "i18n",
    "l10n",
    "locale",
    "locales",
    "third-party",
    "translations",
    "vendored",
];
const MAX_RUNTIME_PATHS: usize = 1_024;
const NON_RUNTIME_SEGMENTS: &[&str] = &[
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
    "testsuite",
    "third_party",
    "vendor",
];
const SOURCE_EXTENSIONS: &[&str] = &[
    "cjs", "cts", "go", "js", "jsx", "mjs", "mts", "py", "rs", "ts", "tsx",
];
const TEST_SEGMENTS: &[&str] = &["__tests__", "test", "tests", "testsuite"];

struct RankedShard {
    has_logic: bool,
    inbound_refs: usize,
    module: AuditModuleInput,
}

struct ShardInputs<'input> {
    all_docs: &'input [String],
    changed_paths: Option<&'input BTreeSet<String>>,
    header_index: &'input HeaderIndex,
    manifests: &'input [String],
    root: &'input Path,
    runtime_paths: &'input [String],
    test_index: &'input TestIndex,
}

/// Discovered module shards plus the repository runtime source closure for sandbox imports.
pub(super) struct DiscoveredAudit {
    pub modules: Vec<AuditModuleInput>,
    pub runtime_paths: Vec<String>,
}

/// Discover bounded production source modules under `root`, optionally filtered by `requested` or
/// `base_ref`.
pub(super) fn discover_modules(
    root: &Path,
    requested: &[PathBuf],
    base_ref: Option<&str>,
) -> Result<DiscoveredAudit, CommandError> {
    let filters = normalize_filters(requested)?;
    let changed_paths = match base_ref {
        Some(reference) => Some(list_changed_files(root, reference)?),
        None => None,
    };
    let all_paths = tenet_engine::list_paths(root)?;
    let manifests = collect_manifests(&all_paths);
    let all_docs = collect_all_docs(&all_paths);
    let test_index = build_test_index(root, &all_paths);
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
        module_files.entry(dir_key).or_default().push(slash_path);
    }
    let header_index = HeaderIndex::build(root, &runtime_paths);
    let shard_inputs = ShardInputs {
        all_docs: &all_docs,
        changed_paths: changed_paths.as_ref(),
        header_index: &header_index,
        manifests: &manifests,
        root,
        runtime_paths: &runtime_paths,
        test_index: &test_index,
    };
    let modules = build_module_shards(merge_tiny_siblings(module_files), &shard_inputs);
    Ok(DiscoveredAudit {
        modules,
        runtime_paths,
    })
}

fn build_module_shards(
    merged_modules: BTreeMap<String, Vec<String>>,
    inputs: &ShardInputs<'_>,
) -> Vec<AuditModuleInput> {
    let mut ranked = Vec::new();
    for (base_name, mut files) in merged_modules {
        files.sort();
        for shard in split_into_shards(inputs.root, &base_name, files) {
            let changed_files = collect_shard_changes(&shard.files, inputs.changed_paths);
            if inputs.changed_paths.is_some() && changed_files.is_empty() {
                continue;
            }
            let docs = select_module_docs(inputs.all_docs, &shard.name, &shard.files);
            let low_test_files = inputs.test_index.find_low_test_files(&shard.files);
            let related_files = find_related_files(inputs.root, &shard.files, inputs.runtime_paths);
            let test_files = inputs
                .test_index
                .find_related_test_files(&shard.name, &shard.files);
            let has_logic = has_executable_logic(inputs.root, &shard.files);
            let inbound_refs = inputs.header_index.count_inbound_refs(&shard.files);
            ranked.push(RankedShard {
                has_logic,
                inbound_refs,
                module: AuditModuleInput {
                    changed_files,
                    docs,
                    files: shard.files,
                    low_test_files,
                    manifests: inputs.manifests.to_vec(),
                    name: shard.name,
                    related_files,
                    test_files,
                    windows: shard.windows,
                },
            });
        }
    }
    sort_ranked_shards(&mut ranked);
    ranked.into_iter().map(|entry| entry.module).collect()
}

fn sort_ranked_shards(ranked: &mut [RankedShard]) {
    ranked.sort_by(|left, right| {
        right
            .module
            .changed_files
            .len()
            .cmp(&left.module.changed_files.len())
            .then_with(|| right.has_logic.cmp(&left.has_logic))
            .then_with(|| {
                right
                    .module
                    .low_test_files
                    .len()
                    .cmp(&left.module.low_test_files.len())
            })
            .then_with(|| right.inbound_refs.cmp(&left.inbound_refs))
            .then_with(|| left.module.name.cmp(&right.module.name))
    });
}

fn build_test_index(root: &Path, paths: &[PathBuf]) -> TestIndex {
    let test_paths: Vec<PathBuf> = paths
        .iter()
        .filter(|path| is_test_source(path))
        .cloned()
        .collect();
    TestIndex::build(root, &test_paths)
}

fn has_hidden_dir(path: &Path) -> bool {
    path.parent().is_some_and(|parent| {
        parent.components().any(|component| {
            matches!(
                component,
                Component::Normal(segment) if segment.to_string_lossy().starts_with('.')
            )
        })
    })
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
    let Some((name, ext)) = read_source_name_and_ext(path, NON_RUNTIME_SEGMENTS) else {
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

fn is_test_source(path: &Path) -> bool {
    let Some((name, ext)) = read_source_name_and_ext(path, &["node_modules", "target", "vendor"])
    else {
        return false;
    };
    has_segment(path, TEST_SEGMENTS) || is_test_filename(name, ext)
}

fn read_module_key(path: &Path) -> String {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => format_slash_path(parent),
        _ => String::from("root"),
    }
}

fn read_source_name_and_ext<'path>(
    path: &'path Path,
    blocked: &[&str],
) -> Option<(&'path str, &'path str)> {
    let ext = path.extension().and_then(|item| item.to_str())?;
    if !SOURCE_EXTENSIONS.contains(&ext) || has_hidden_dir(path) || has_segment(path, blocked) {
        return None;
    }
    let name = path.file_name().and_then(|item| item.to_str())?;
    Some((name, ext))
}
