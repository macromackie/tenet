use alloc::collections::BTreeMap;
use std::{fs, path::Path};

const MAX_SHARD_BYTES: u64 = 100_000;
const MAX_SHARD_FILES: usize = 10;
const MERGE_MAX_LEAF_FILES: usize = 2;
const MERGE_MIN_SIBLINGS: usize = 4;

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

/// Partition one module's sorted source paths into size- and file-count-bounded shards.
pub(super) fn split_into_shards(
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
