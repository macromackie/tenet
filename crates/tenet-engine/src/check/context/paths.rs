use alloc::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Map, Value};

use super::{CombinedError, scope::Scope};

pub(super) const ENCODING: &str =
    "The supplied_source and omitted_source trees partition the current scoped inventory. \
     Directory keys end in / and prepend descendants. Arrays list filenames; an empty key lists \
     files in the current directory. Omitted source contents are not supplied.";

const COUNTS_ENCODING: &str = "supplied_source is a directory tree of the supplied scoped files: \
                               directory keys end in / and prepend descendants, arrays list \
                               filenames, and an empty key lists files in the current directory. \
                               omitted_source_counts maps directory prefixes to how many further \
                               scoped files are beneath them; those paths and contents are not \
                               supplied. An empty prefix covers the whole scope.";

const MAX_DIRECTORY_DEPTH: usize = 32;

pub(super) fn collect_metadata(scope: &Scope<'_>) -> Result<Map<String, Value>, CombinedError> {
    let expanded = Map::from_iter([
        ("inventory".into(), serde_json::json!(scope.inventory)),
        ("omitted_source".into(), serde_json::json!(scope.omitted)),
    ]);
    let compact = Map::from_iter([(
        "inventory".into(),
        serde_json::json!({
            "encoding": ENCODING,
            "supplied_source": encode(&scope.supplied_inventory)?,
            "omitted_source": encode(&scope.omitted)?,
        }),
    )]);
    if serde_json::to_vec(&compact)?.len() < serde_json::to_vec(&expanded)?.len() {
        Ok(compact)
    } else {
        Ok(expanded)
    }
}

/// The next directory depth at which to count omitted inventory files, coarser than `current`;
/// `None` once every omitted file is counted in a single total.
pub(super) fn find_coarser(scope: &Scope<'_>, current: Option<usize>) -> Option<usize> {
    match current {
        Some(depth) => depth.checked_sub(1),
        None => {
            scope
                .omitted
                .iter()
                .map(|path| path.components().count().saturating_sub(1))
                .max()
        },
    }
}

/// Inventory metadata that lists supplied paths exactly and counts omitted files per directory
/// prefix of at most `depth` directories.
#[expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "`split` returns at least one segment, so the length is at least 1; the counter is \
              bounded by the omitted path count."
)]
pub(super) fn count_paths(
    scope: &Scope<'_>,
    depth: usize,
) -> Result<Map<String, Value>, CombinedError> {
    let mut counts = BTreeMap::<String, usize>::new();
    for path in &scope.omitted {
        let text = path.to_str().ok_or(CombinedError::NotUtf8Path)?;
        let directories: Vec<_> = text.split('/').collect();
        let prefix: String = directories[..depth.min(directories.len() - 1)].iter().fold(
            String::new(),
            |mut prefix, directory| {
                prefix.push_str(directory);
                prefix.push('/');
                prefix
            },
        );
        *counts.entry(prefix).or_default() += 1;
    }
    Ok(Map::from_iter([(
        "inventory".into(),
        serde_json::json!({
            "encoding": COUNTS_ENCODING,
            "supplied_source": encode(&scope.supplied_inventory)?,
            "omitted_source_counts": counts,
        }),
    )]))
}

#[derive(Default)]
struct Directory<'text> {
    directories: BTreeMap<&'text str, Self>,
    files: Vec<&'text str>,
}

impl<'text> Directory<'text> {
    fn extend_flat(self, prefix: &str, entries: &mut Map<String, Value>) {
        if !self.files.is_empty() {
            drop(entries.insert(prefix.to_owned(), serde_json::json!(self.files)));
        }
        for (name, directory) in self.directories {
            directory.extend_flat(&format!("{prefix}{name}/"), entries);
        }
    }

    fn insert(&mut self, path: &'text str) {
        if let Some((directory, remaining)) = path.split_once('/') {
            self.directories
                .entry(directory)
                .or_default()
                .insert(remaining);
        } else {
            self.files.push(path);
        }
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The depth is bounded by the number of directories in a repository path."
    )]
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    fn into_value(self, depth: usize) -> Value {
        if self.directories.is_empty() {
            return serde_json::json!(self.files);
        }
        let mut entries = Map::new();
        if depth == MAX_DIRECTORY_DEPTH {
            self.extend_flat("", &mut entries);
            return Value::Object(entries);
        }
        if !self.files.is_empty() {
            drop(entries.insert(String::new(), serde_json::json!(self.files)));
        }
        for (name, mut directory) in self.directories {
            let mut prefix = format!("{name}/");
            while directory.files.is_empty() && directory.directories.len() == 1 {
                if let Some((child_name, child)) = directory.directories.pop_first() {
                    prefix.push_str(child_name);
                    prefix.push('/');
                    directory = child;
                }
            }
            drop(entries.insert(prefix, directory.into_value(depth + 1)));
        }
        Value::Object(entries)
    }
}

pub(super) fn encode(paths: &[&PathBuf]) -> Result<Value, CombinedError> {
    let mut root = Directory::default();
    for path in paths {
        root.insert(path.to_str().ok_or(CombinedError::NotUtf8Path)?);
    }
    Ok(root.into_value(0))
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    reason = "Unit tests stay beside the private functions they test."
)]
mod tests {
    use alloc::collections::BTreeSet;
    use std::path::PathBuf;

    use anyhow::{Result, ensure};
    use serde_json::Value;

    use super::encode;

    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The test waits for one kind and ignores every other."
    )]
    fn decode(value: &Value, prefix: &str, paths: &mut BTreeSet<PathBuf>) -> Result<()> {
        match value {
            Value::Array(files) => {
                for file in files {
                    let name = file
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("invalid filename"))?;
                    ensure!(!name.contains('/'), "filename contains a directory");
                    ensure!(
                        paths.insert(format!("{prefix}{name}").into()),
                        "duplicate path"
                    );
                }
            },
            Value::Object(directories) => {
                for (directory, files) in directories {
                    ensure!(
                        directory.is_empty() || directory.ends_with('/'),
                        "invalid directory"
                    );
                    decode(files, &format!("{prefix}{directory}"), paths)?;
                }
            },
            _ => anyhow::bail!("invalid tree"),
        }
        Ok(())
    }

    #[test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "A test reports its failure by panicking."
    )]
    fn directory_encoding_keeps_exact_paths_without_prefix_or_name_collisions() -> Result<()> {
        for names in [
            Vec::new(),
            vec!["README.md"],
            vec![
                "a",
                "a.rs",
                "a/deep/path/a.rs",
                "a/deep/path/b.rs",
                "a/deep/other.rs",
                "a-extra/a.rs",
                "src/a.rs",
                "src/files",
                "src/empty",
                "src/\u{3b4}\\\\x\"\n.rs",
            ],
        ] {
            let paths: Vec<PathBuf> = names.into_iter().map(PathBuf::from).collect();
            let encoded = encode(&paths.iter().collect::<Vec<_>>())?;
            let encoded_bytes = serde_json::to_vec(&encoded)?;
            let parsed = serde_json::from_slice(&encoded_bytes)?;
            let mut decoded_paths = BTreeSet::new();
            decode(&parsed, "", &mut decoded_paths)?;
            assert_eq!(decoded_paths, paths.into_iter().collect());
        }
        Ok(())
    }

    #[test]
    #[expect(
        clippy::indexing_slicing,
        reason = "A missing element panics, and the test harness reports that panic as this \
                  test's failure."
    )]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "A test reports its failure by panicking."
    )]
    fn deeply_branching_inventory_stays_within_provider_json_depth() -> Result<()> {
        let mut directory = String::new();
        let mut paths = Vec::new();
        for _ in 0..130_u32 {
            directory.push_str("d/");
            paths.push(PathBuf::from(format!("{directory}f.rs")));
        }
        let tree = encode(&paths.iter().collect::<Vec<_>>())?;
        let state = serde_json::json!({"inventory": {
            "encoding": super::ENCODING,
            "supplied_source": ["selected.rs"],
            "omitted_source": tree,
        }});
        let parsed = ev_grep_core::parse_json(&serde_json::to_vec(&state)?)?;
        let mut decoded_paths = BTreeSet::new();
        decode(
            &parsed["inventory"]["omitted_source"],
            "",
            &mut decoded_paths,
        )?;
        assert_eq!(decoded_paths, paths.into_iter().collect());
        Ok(())
    }
}
