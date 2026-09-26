use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use tenet_engine::{ContractStatus, Plan, PreparedSnapshot, Selection};

use crate::cli::Eval;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    cases: Vec<Case>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Case {
    pub name: String,
    pub patch: Option<PathBuf>,
    pub contract: Option<String>,
    #[serde(default)]
    pub context: Vec<PathBuf>,
    #[serde(default = "development")]
    pub split: String,
    pub expect: BTreeMap<String, ContractStatus>,
}

fn development() -> String {
    "development".into()
}

pub(crate) struct FixtureCase {
    pub id: String,
    pub root: PathBuf,
    pub case: Case,
}

pub(crate) struct Prepared {
    pub snapshot: PreparedSnapshot,
    pub plan: Plan,
    pub hash: String,
}

pub(crate) fn discover(path: &Path, args: &Eval) -> Result<Vec<FixtureCase>> {
    let path = path.canonicalize().context("invalid fixture path")?;
    let mut manifests = Vec::new();
    if path.is_file() {
        ensure!(
            path.file_name().is_some_and(|n| n == "eval.toml"),
            "expected eval.toml"
        );
        manifests.push(path.clone());
    } else {
        find(&path, &mut manifests)?;
    }
    manifests.sort();
    let single = manifests.len() == 1;
    let mut cases = Vec::new();
    for manifest in manifests {
        let root = manifest.parent().context("fixture has no parent")?;
        let prefix = if single {
            root.file_name()
                .context("fixture has no name")?
                .to_string_lossy()
                .into_owned()
        } else {
            root.strip_prefix(&path)?
                .to_string_lossy()
                .replace('\\', "/")
        };
        let config: Manifest = toml::from_str(&fs::read_to_string(&manifest)?)
            .with_context(|| format!("invalid {}", manifest.display()))?;
        ensure!(!config.cases.is_empty(), "empty fixture: {prefix}");
        let mut names = BTreeSet::new();
        for mut case in config.cases {
            ensure!(
                !case.name.is_empty()
                    && case
                        .name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c)),
                "invalid case name"
            );
            ensure!(
                names.insert(case.name.clone()),
                "duplicate case: {}",
                case.name
            );
            ensure!(
                ["development", "heldout"].contains(&case.split.as_str()),
                "invalid split"
            );
            let id = format!("{prefix}/{}", case.name);
            if args.split != "all" && case.split != args.split {
                continue;
            }
            if args
                .case
                .as_ref()
                .is_some_and(|n| n != &case.name && n != &id)
            {
                continue;
            }
            if let Some(contract) = &args.run.contract {
                if !case.expect.contains_key(contract) {
                    continue;
                }
                case.contract = Some(contract.clone());
                case.expect.retain(|name, _| name == contract);
            }
            ensure!(!case.expect.is_empty(), "no expectations: {id}");
            cases.push(FixtureCase {
                id,
                root: root.to_owned(),
                case,
            });
        }
    }
    ensure!(!cases.is_empty(), "no fixture cases selected");
    Ok(cases)
}

fn find(root: &Path, manifests: &mut Vec<PathBuf>) -> Result<()> {
    if root.join("eval.toml").is_file() {
        manifests.push(root.join("eval.toml"));
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if entry.file_type()?.is_dir()
            && !name.starts_with('.')
            && !["node_modules", "target", "results"].contains(&name.as_ref())
        {
            find(&entry.path(), manifests)?;
        }
    }
    Ok(())
}

impl FixtureCase {
    pub(crate) fn prepare(&self) -> Result<Prepared> {
        let base = contained(&self.root, Path::new("base"))?;
        let patch = self
            .case
            .patch
            .as_ref()
            .map(|p| contained(&self.root, p))
            .transpose()?;
        let snapshot = PreparedSnapshot::new(&base, patch.as_deref())?;
        let plan = tenet_engine::plan(
            snapshot.root(),
            &Selection {
                base: patch.as_ref().map(|_| "HEAD".into()),
                contract: self.case.contract.clone(),
                ..Selection::default()
            },
        )?;
        let plan = plan.with_context(&self.case.context)?;
        let actual: BTreeSet<_> = plan.contracts.iter().map(|c| &c.name).collect();
        let expected: BTreeSet<_> = self.case.expect.keys().collect();
        ensure!(
            actual == expected,
            "{}: expectations must name every selected contract exactly",
            self.id
        );
        for status in self.case.expect.values() {
            ensure!(
                if patch.is_some() {
                    *status != ContractStatus::Verified
                } else {
                    !matches!(
                        status,
                        ContractStatus::Preserved | ContractStatus::Unaffected
                    )
                },
                "{}: expectation does not match full/diff mode",
                self.id
            );
        }
        let mut hash = blake3::Hasher::new();
        hash.update(&fs::read(self.root.join("eval.toml"))?);
        if let Some(patch_hash) = &snapshot.patch_hash {
            hash.update(patch_hash.as_bytes());
        }
        for file in &plan.inventory {
            hash.update(file.to_string_lossy().as_bytes());
            hash.update(&fs::read(plan.root.join(file))?);
        }
        for contract in &plan.contracts {
            hash.update(contract.hash.as_bytes());
        }
        Ok(Prepared {
            snapshot,
            plan,
            hash: hash.finalize().to_hex().to_string(),
        })
    }
}

fn contained(root: &Path, path: &Path) -> Result<PathBuf> {
    ensure!(
        path.components().all(|p| matches!(p, Component::Normal(_))),
        "fixture path must stay within its directory"
    );
    let resolved = root
        .join(path)
        .canonicalize()
        .context("missing fixture input")?;
    ensure!(
        resolved.starts_with(root),
        "fixture input escapes its directory"
    );
    Ok(resolved)
}

#[cfg(test)]
mod tests;
