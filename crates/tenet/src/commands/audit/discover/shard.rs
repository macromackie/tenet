#[cfg(test)]
mod tests;

use alloc::collections::BTreeMap;
use core::mem;
use std::{fs, path::Path};

use tenet_checks::behavior::AuditFileWindow;

const DEFINITION_PREFIXES: &[&str] = &[
    "async def ",
    "class ",
    "def ",
    "export ",
    "fn ",
    "func ",
    "function ",
    "impl ",
    "pub ",
    "type ",
];
const MAX_SHARD_BYTES: u64 = 100_000;
const MAX_SHARD_FILES: usize = 10;
const MAX_WINDOW_BYTES: usize = 48_000;
const MAX_WINDOWED_FILE_BYTES: u64 = 512_000;
const MERGE_MAX_LEAF_FILES: usize = 2;
const MERGE_MIN_SIBLINGS: usize = 4;
const MIN_SPLIT_WINDOW_BYTES: usize = 32_000;

/// One bounded module shard, optionally scoped to semantic line windows of an oversized file.
pub(super) struct ShardSpec {
    pub files: Vec<String>,
    pub name: String,
    pub windows: Vec<AuditFileWindow>,
}

struct PendingBucket {
    files: Vec<String>,
    windows: Vec<AuditFileWindow>,
}

/// Collapse tiny sibling leaf directories under their shared parent before sharding.
pub(super) fn merge_tiny_siblings(
    module_files: BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, Vec<String>> {
    let mut parent_small_counts: BTreeMap<String, usize> = BTreeMap::new();
    for (dir_key, files) in &module_files {
        if files.len() <= MERGE_MAX_LEAF_FILES
            && let Some((parent, _)) = dir_key.rsplit_once('/')
        {
            let count = parent_small_counts.entry(parent.to_owned()).or_insert(0);
            *count = count.saturating_add(1);
        }
    }
    let mut merged_modules: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (dir_key, files) in module_files {
        let can_merge = files.len() <= MERGE_MAX_LEAF_FILES;
        let target_key = if can_merge
            && let Some((parent, _)) = dir_key.rsplit_once('/')
            && parent_small_counts
                .get(parent)
                .is_some_and(|count| *count >= MERGE_MIN_SIBLINGS)
        {
            parent.to_owned()
        } else {
            dir_key
        };
        merged_modules.entry(target_key).or_default().extend(files);
    }
    merged_modules
}

/// Partition one module's sorted source paths into bounded shards, windowing oversized files.
pub(super) fn split_into_shards(
    root: &Path,
    base_name: &str,
    files: Vec<String>,
) -> Vec<ShardSpec> {
    let mut buckets: Vec<PendingBucket> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_bytes: u64 = 0;
    for file in files {
        let size = fs::metadata(root.join(&file)).map_or(0, |meta| meta.len());
        if let Some(windows) = build_file_windows(root, &file, size) {
            flush_current_bucket(&mut buckets, &mut current, &mut current_bytes);
            for window in windows {
                buckets.push(PendingBucket {
                    files: vec![file.clone()],
                    windows: vec![window],
                });
            }
            continue;
        }
        let exceeds_files = current.len() >= MAX_SHARD_FILES;
        let exceeds_bytes =
            !current.is_empty() && current_bytes.saturating_add(size) > MAX_SHARD_BYTES;
        if exceeds_files || exceeds_bytes {
            flush_current_bucket(&mut buckets, &mut current, &mut current_bytes);
        }
        current.push(file);
        current_bytes = current_bytes.saturating_add(size);
    }
    flush_current_bucket(&mut buckets, &mut current, &mut current_bytes);
    format_shard_buckets(base_name, buckets)
}

fn build_file_windows(root: &Path, file: &str, size: u64) -> Option<Vec<AuditFileWindow>> {
    let threshold = u64::try_from(MAX_WINDOW_BYTES).ok()?;
    if size <= threshold || size > MAX_WINDOWED_FILE_BYTES {
        return None;
    }
    let content = fs::read_to_string(root.join(file)).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return None;
    }
    let ranges = split_line_windows(&lines);
    if ranges.len() <= 1 {
        return None;
    }
    Some(
        ranges
            .into_iter()
            .map(|(start_line, end_line)| {
                AuditFileWindow {
                    end_line,
                    path: file.to_owned(),
                    start_line,
                }
            })
            .collect(),
    )
}

fn flush_current_bucket(
    buckets: &mut Vec<PendingBucket>,
    current: &mut Vec<String>,
    current_bytes: &mut u64,
) {
    if current.is_empty() {
        return;
    }
    buckets.push(PendingBucket {
        files: mem::take(current),
        windows: Vec::new(),
    });
    *current_bytes = 0;
}

fn format_shard_buckets(base_name: &str, buckets: Vec<PendingBucket>) -> Vec<ShardSpec> {
    if buckets.len() <= 1 {
        return buckets
            .into_iter()
            .map(|bucket| {
                ShardSpec {
                    files: bucket.files,
                    name: base_name.to_owned(),
                    windows: bucket.windows,
                }
            })
            .collect();
    }
    buckets
        .into_iter()
        .enumerate()
        .map(|(index, bucket)| {
            let number = index.saturating_add(1);
            ShardSpec {
                files: bucket.files,
                name: format!("{base_name}#{number}"),
                windows: bucket.windows,
            }
        })
        .collect()
}

fn is_definition_boundary(line: &str) -> bool {
    DEFINITION_PREFIXES
        .iter()
        .any(|prefix| line.starts_with(prefix))
}

fn split_line_windows(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start_line = 1_usize;
    let mut used_bytes = 0_usize;
    for (index, line) in lines.iter().enumerate() {
        let line_number = index.saturating_add(1);
        let line_bytes = line.len().saturating_add(1);
        let soft_boundary = used_bytes >= MIN_SPLIT_WINDOW_BYTES && is_definition_boundary(line);
        let hard_boundary =
            line_number > start_line && used_bytes.saturating_add(line_bytes) > MAX_WINDOW_BYTES;
        if soft_boundary || hard_boundary {
            ranges.push((start_line, line_number.saturating_sub(1)));
            start_line = line_number;
            used_bytes = 0;
        }
        used_bytes = used_bytes.saturating_add(line_bytes);
    }
    if start_line <= lines.len() {
        ranges.push((start_line, lines.len()));
    }
    ranges
}
