use alloc::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use super::super::context::{format_slash_path, read_bounded_text};

const ENTRYPOINT_LIMIT: usize = 2;
const INVALID_PATH_SEGMENTS: &[&str] = &["", ".", ".."];
const JS_EXTENSIONS: &[&str] = &["cjs", "cts", "js", "jsx", "mjs", "mts", "ts", "tsx"];
const JS_STANDARD_ENTRYPOINTS: &[&str] = &[
    "src/index.ts",
    "src/index.mts",
    "src/index.tsx",
    "src/index.js",
    "src/index.mjs",
    "lib/index.js",
    "lib/index.mjs",
    "lib/index.cjs",
    "index.js",
    "index.ts",
];
const MAX_MANIFEST_BYTES: usize = 64_000;

struct EntrypointLookup<'lookup> {
    entrypoints: Vec<String>,
    included_paths: &'lookup BTreeSet<&'lookup str>,
    runtime_paths: BTreeSet<&'lookup str>,
}

impl<'lookup> EntrypointLookup<'lookup> {
    fn build(
        included_paths: &'lookup BTreeSet<&'lookup str>,
        runtime_paths: &'lookup [String],
    ) -> Self {
        Self {
            entrypoints: Vec::new(),
            included_paths,
            runtime_paths: runtime_paths.iter().map(String::as_str).collect(),
        }
    }

    fn is_eligible(&self, candidate: &str) -> bool {
        !self.included_paths.contains(candidate)
            && self.runtime_paths.contains(candidate)
            && !self
                .entrypoints
                .iter()
                .any(|existing| existing == candidate)
    }

    const fn is_full(&self) -> bool {
        self.entrypoints.len() >= ENTRYPOINT_LIMIT
    }

    fn push_if_runtime(&mut self, candidate: &str) {
        if self.is_full() || !self.is_eligible(candidate) {
            return;
        }
        self.entrypoints.push(candidate.to_owned());
    }
}

/// Detect up to two public package or crate entrypoints present in `runtime_paths` for
/// `shard_files`.
pub(super) fn find_package_entrypoints(
    root: &Path,
    shard_files: &[String],
    runtime_paths: &[String],
    included_paths: &BTreeSet<&str>,
) -> Vec<String> {
    let mut lookup = EntrypointLookup::build(included_paths, runtime_paths);
    if has_shard_extension(shard_files, &["rs"]) {
        collect_rust_entrypoints(shard_files, &mut lookup);
    }
    if has_shard_extension(shard_files, JS_EXTENSIONS) {
        collect_js_entrypoints(root, &mut lookup);
    }
    if has_shard_extension(shard_files, &["py"]) {
        collect_python_entrypoints(shard_files, &mut lookup);
    }
    if has_shard_extension(shard_files, &["go"]) {
        collect_go_entrypoints(root, shard_files, &mut lookup);
    }
    lookup.entrypoints
}

/// Return true when `path` has a lowercase or exact extension listed in `extensions`.
pub(super) fn has_path_extension(path: &str, extensions: &[&str]) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| extensions.contains(&ext))
}

/// Return the slash-separated parent directory of `path`, or empty for root-level files.
pub(super) fn read_parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn collect_ancestor_dirs(path: &str) -> Vec<String> {
    let mut dirs = Vec::new();
    let mut current = read_parent_dir(path);
    while !current.is_empty() {
        dirs.push(current.to_owned());
        current = read_parent_dir(current);
    }
    dirs.reverse();
    dirs
}

fn collect_go_entrypoints(root: &Path, shard_files: &[String], lookup: &mut EntrypointLookup<'_>) {
    let has_subpackage_go = shard_files
        .iter()
        .any(|path| has_path_extension(path, &["go"]) && !read_parent_dir(path).is_empty());
    if !has_subpackage_go {
        return;
    }
    if let Some(leaf) = read_go_module_leaf(root) {
        lookup.push_if_runtime(&format!("{leaf}.go"));
    }
}

fn collect_js_entrypoints(root: &Path, lookup: &mut EntrypointLookup<'_>) {
    collect_package_json_entrypoints(root, lookup);
    for candidate in JS_STANDARD_ENTRYPOINTS {
        if lookup.is_full() {
            break;
        }
        lookup.push_if_runtime(candidate);
    }
}

fn collect_package_json_entrypoints(root: &Path, lookup: &mut EntrypointLookup<'_>) {
    let text = read_bounded_text(&root.join("package.json"), MAX_MANIFEST_BYTES);
    let Ok(manifest) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    for field in ["main", "module"] {
        if let Some(raw) = manifest.get(field).and_then(Value::as_str) {
            push_manifest_entry(raw, lookup);
        }
    }
    let Some(name) = manifest.get("name").and_then(Value::as_str) else {
        return;
    };
    let Some(stem) = read_unscoped_package_stem(name) else {
        return;
    };
    for candidate in [
        format!("lib/{stem}.js"),
        format!("lib/{stem}.mjs"),
        format!("lib/{stem}.ts"),
        format!("src/{stem}.ts"),
        format!("src/{stem}.js"),
    ] {
        lookup.push_if_runtime(&candidate);
    }
}

fn collect_python_entrypoints(shard_files: &[String], lookup: &mut EntrypointLookup<'_>) {
    for path in shard_files {
        if !has_path_extension(path, &["py"]) {
            continue;
        }
        let candidates: Vec<String> = collect_ancestor_dirs(path)
            .into_iter()
            .map(|dir| format!("{dir}/__init__.py"))
            .filter(|candidate| lookup.is_eligible(candidate))
            .collect();
        if let Some(first) = candidates.first() {
            lookup.push_if_runtime(first);
        }
        if let Some(last) = candidates.last() {
            lookup.push_if_runtime(last);
        }
        if lookup.is_full() {
            break;
        }
    }
}

fn collect_rust_entrypoints(shard_files: &[String], lookup: &mut EntrypointLookup<'_>) {
    let mut visited_dirs = BTreeSet::new();
    for path in shard_files {
        if has_path_extension(path, &["rs"]) {
            walk_rust_ancestors(read_parent_dir(path), &mut visited_dirs, lookup);
        }
    }
}

fn extract_go_module_leaf(module_path: &str) -> Option<String> {
    let mut segments: Vec<&str> = module_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.len() > 1
        && segments
            .last()
            .is_some_and(|segment| is_go_major_version_segment(segment))
    {
        let _removed = segments.pop();
    }
    let leaf = *segments.last()?;
    is_safe_path_segment(leaf).then(|| leaf.to_owned())
}

fn format_rust_lib_path(dir: &str) -> String {
    if dir.is_empty() {
        String::from("src/lib.rs")
    } else {
        format!("{dir}/src/lib.rs")
    }
}

fn has_shard_extension(shard_files: &[String], extensions: &[&str]) -> bool {
    shard_files
        .iter()
        .any(|path| has_path_extension(path, extensions))
}

fn is_go_major_version_segment(segment: &str) -> bool {
    segment.strip_prefix('v').is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn is_safe_path_segment(segment: &str) -> bool {
    !INVALID_PATH_SEGMENTS.contains(&segment) && !segment.contains('\\')
}

fn normalize_manifest_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.contains("..") {
        return None;
    }
    let normalized = format_slash_path(Path::new(trimmed));
    (!normalized.is_empty()).then_some(normalized)
}

fn push_manifest_entry(raw: &str, lookup: &mut EntrypointLookup<'_>) {
    let Some(normalized) = normalize_manifest_path(raw) else {
        return;
    };
    let before = lookup.entrypoints.len();
    lookup.push_if_runtime(&normalized);
    if lookup.entrypoints.len() > before || Path::new(&normalized).extension().is_some() {
        return;
    }
    for ext in ["js", "mjs", "cjs", "ts"] {
        lookup.push_if_runtime(&format!("{normalized}.{ext}"));
        if lookup.entrypoints.len() > before {
            break;
        }
    }
}

fn read_go_module_leaf(root: &Path) -> Option<String> {
    let text = read_bounded_text(&root.join("go.mod"), MAX_MANIFEST_BYTES);
    for line in text.lines() {
        let without_comment = line.split("//").next().unwrap_or_default().trim();
        let Some(rest) = without_comment.strip_prefix("module") else {
            continue;
        };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let module_path = rest.split_whitespace().next()?.trim_matches(['"', '`']);
        return extract_go_module_leaf(module_path);
    }
    None
}

fn read_unscoped_package_stem(name: &str) -> Option<&str> {
    let trimmed = name.trim();
    let stem = trimmed.rsplit_once('/').map_or(trimmed, |(_, leaf)| leaf);
    is_safe_path_segment(stem).then_some(stem)
}

fn walk_rust_ancestors<'path>(
    start_dir: &'path str,
    visited_dirs: &mut BTreeSet<&'path str>,
    lookup: &mut EntrypointLookup<'_>,
) {
    let mut dir = start_dir;
    loop {
        if visited_dirs.insert(dir) {
            let candidate = format_rust_lib_path(dir);
            lookup.push_if_runtime(&candidate);
        }
        if dir.is_empty() || lookup.is_full() {
            break;
        }
        dir = read_parent_dir(dir);
    }
}
