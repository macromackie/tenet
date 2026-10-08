use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use super::scope::Scope;

pub(super) const ENCODING: &str = "The supplied_source and omitted_source trees partition the current scoped inventory. Directory keys end in / and prepend descendants. Arrays list filenames; an empty key lists files in the current directory. Omitted source contents are not supplied.";

const MAX_DIRECTORY_DEPTH: usize = 32;

pub(super) fn metadata(scope: &Scope<'_>) -> Result<Map<String, Value>> {
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

#[derive(Default)]
struct Directory<'a> {
    files: Vec<&'a str>,
    directories: BTreeMap<&'a str, Directory<'a>>,
}

impl<'a> Directory<'a> {
    fn insert(&mut self, path: &'a str) {
        if let Some((directory, remaining)) = path.split_once('/') {
            self.directories
                .entry(directory)
                .or_default()
                .insert(remaining);
        } else {
            self.files.push(path);
        }
    }

    fn into_value(self, depth: usize) -> Value {
        if self.directories.is_empty() {
            return serde_json::json!(self.files);
        }
        let mut entries = Map::new();
        if depth == MAX_DIRECTORY_DEPTH {
            self.flatten(String::new(), &mut entries);
            return Value::Object(entries);
        }
        if !self.files.is_empty() {
            entries.insert(String::new(), serde_json::json!(self.files));
        }
        for (name, mut directory) in self.directories {
            let mut prefix = format!("{name}/");
            while directory.files.is_empty() && directory.directories.len() == 1 {
                if let Some((name, child)) = directory.directories.pop_first() {
                    prefix.push_str(name);
                    prefix.push('/');
                    directory = child;
                }
            }
            entries.insert(prefix, directory.into_value(depth + 1));
        }
        Value::Object(entries)
    }

    fn flatten(self, prefix: String, entries: &mut Map<String, Value>) {
        if !self.files.is_empty() {
            entries.insert(prefix.clone(), serde_json::json!(self.files));
        }
        for (name, directory) in self.directories {
            directory.flatten(format!("{prefix}{name}/"), entries);
        }
    }
}

pub(super) fn encode(paths: &[&PathBuf]) -> Result<Value> {
    let mut root = Directory::default();
    for path in paths {
        root.insert(path.to_str().context("inventory path is not UTF-8")?);
    }
    Ok(root.into_value(0))
}

#[cfg(test)]
mod tests {
    use super::encode;
    use anyhow::{Result, ensure};
    use serde_json::Value;
    use std::{collections::BTreeSet, path::PathBuf};

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
            }
            Value::Object(directories) => {
                for (directory, files) in directories {
                    ensure!(
                        directory.is_empty() || directory.ends_with('/'),
                        "invalid directory"
                    );
                    decode(files, &format!("{prefix}{directory}"), paths)?;
                }
            }
            _ => anyhow::bail!("invalid tree"),
        }
        Ok(())
    }

    #[test]
    fn directory_encoding_preserves_exact_paths_without_prefix_or_filename_collisions() -> Result<()>
    {
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
                "src/δ\\\\x\"\n.rs",
            ],
        ] {
            let paths: Vec<PathBuf> = names.into_iter().map(PathBuf::from).collect();
            let encoded = encode(&paths.iter().collect::<Vec<_>>())?;
            let encoded_bytes = serde_json::to_vec(&encoded)?;
            let encoded = serde_json::from_slice(&encoded_bytes)?;
            let mut decoded = BTreeSet::new();
            decode(&encoded, "", &mut decoded)?;
            assert_eq!(decoded, paths.into_iter().collect());
        }
        Ok(())
    }

    #[test]
    fn deeply_branching_inventory_stays_within_provider_json_depth() -> Result<()> {
        let mut directory = String::new();
        let mut paths = Vec::new();
        for _ in 0..130 {
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
        let mut decoded = BTreeSet::new();
        decode(&parsed["inventory"]["omitted_source"], "", &mut decoded)?;
        assert_eq!(decoded, paths.into_iter().collect());
        Ok(())
    }
}
