use alloc::collections::BTreeMap;
use std::{
    fs,
    io::{self, Read as _, Write as _},
    path::{Path, PathBuf},
};

use similar::TextDiff;
use tempfile::{NamedTempFile, TempDir};
use tenet_contracts::{Contract, hash_file};

use super::{Change, Job, Tool};

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SOURCE_BYTES: usize = 64 * 1024 * 1024;

pub(super) struct Staging<'root> {
    checkout: TempDir,
    inputs: BTreeMap<PathBuf, String>,
    originals: BTreeMap<PathBuf, String>,
    root: &'root Path,
    source_bytes: usize,
    tree: PathBuf,
}

fn read_source(root: &Path, path: &Path) -> io::Result<String> {
    let file = root.join(path);
    if file.canonicalize()? != file || !file.symlink_metadata()?.is_file() {
        return Err(io::Error::other(format!(
            "fix requires a regular source file: {:?}",
            path.display()
        )));
    }
    let mut text = String::new();
    let size = fs::File::open(&file)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_string(&mut text)?;
    if u64::try_from(size).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
        return Err(io::Error::other(format!(
            "fix input exceeds 8 MiB: {:?}",
            path.display()
        )));
    }
    Ok(text)
}

impl<'root> Staging<'root> {
    pub(super) fn apply(self) -> io::Result<Vec<Change>> {
        let mut changes = Vec::new();
        let mut pending = Vec::new();
        // Preflight all captured dependencies. Conflicting editor writes should never get replaced.
        self.check_sources()?;
        for (path, original) in &self.originals {
            let updated = read_source(&self.tree, path)?;
            if updated == *original {
                continue;
            }
            let path_name = path.to_string_lossy();
            let patch = TextDiff::from_lines(original.as_str(), &updated)
                .unified_diff()
                .header(&format!("a/{path_name}"), &format!("b/{path_name}"))
                .to_string();
            let destination = self.root.join(path);
            let parent = destination
                .parent()
                .ok_or_else(|| io::Error::other("missing source directory"))?;
            let mut ready = NamedTempFile::new_in(parent)?;
            ready.write_all(updated.as_bytes())?;
            ready
                .as_file()
                .set_permissions(destination.metadata()?.permissions())?;
            pending.push((path, original, ready));
            changes.push(Change {
                patch,
                path: path.clone(),
            });
        }
        for (index, (path, original, ready)) in pending.into_iter().enumerate() {
            if &read_source(self.root, path)? != original {
                return Err(io::Error::other(format!(
                    "{:?} changed during fix. {index:?} files already updated",
                    path.display()
                )));
            }
            let _written = ready.persist(self.root.join(path)).map_err(|error| {
                io::Error::new(
                    error.error.kind(),
                    format!(
                        "cannot replace {:?} ({index:?} files already updated): {:#}",
                        path.display(),
                        error.error
                    ),
                )
            })?;
        }
        Ok(changes)
    }

    fn capture(&mut self, path: &Path) -> io::Result<()> {
        if self.originals.contains_key(path) {
            return Ok(());
        }
        let text = read_source(self.root, path)?;
        let source_bytes = self.source_bytes.saturating_add(text.len());
        if source_bytes > MAX_SOURCE_BYTES {
            return Err(io::Error::other(
                "fix inputs exceed 64 MiB; select a narrower path",
            ));
        }
        let staged = self.tree.join(path);
        let parent = staged
            .parent()
            .ok_or_else(|| io::Error::other("missing source directory"))?;
        fs::create_dir_all(parent)?;
        fs::write(staged, &text)?;
        self.source_bytes = source_bytes;
        let _previous = self.originals.insert(path.to_owned(), text);
        Ok(())
    }

    pub(super) fn check_sources(&self) -> io::Result<()> {
        for (path, original) in self.originals.iter().chain(&self.inputs) {
            if &read_source(self.root, path)? != original {
                return Err(io::Error::other(format!(
                    "{:?} changed during native checks",
                    path.display()
                )));
            }
        }
        Ok(())
    }

    pub(super) fn new(root: &'root Path) -> io::Result<Self> {
        let checkout = tempfile::tempdir()?;
        let tree = checkout.path().canonicalize()?.join("source");
        fs::create_dir_all(&tree)?;
        Ok(Self {
            checkout,
            inputs: BTreeMap::new(),
            originals: BTreeMap::new(),
            root,
            source_bytes: 0,
            tree,
        })
    }

    pub(super) fn prepare_job(
        &mut self,
        contract: &Contract,
        tool: Tool,
        digest: &str,
        index: usize,
        targets: Vec<PathBuf>,
    ) -> io::Result<Option<Job>> {
        let Some(config) = self.write_config(contract, tool, digest, index)? else {
            return Ok(None);
        };
        for path in &targets {
            self.capture(path)?;
        }
        Ok(Some(Job::new(contract.name.clone(), tool, config, targets)))
    }

    pub(super) fn tree(&self) -> &Path {
        &self.tree
    }

    fn write_config(
        &mut self,
        contract: &Contract,
        tool: Tool,
        digest: &str,
        index: usize,
    ) -> io::Result<Option<PathBuf>> {
        let folder = contract
            .path
            .parent()
            .ok_or_else(|| io::Error::other("missing contract folder"))?;
        let asset = folder.join(tool.find_asset_path());
        let content = read_source(self.root, &asset)?;
        if hash_file(content.as_bytes()) != digest {
            return Err(io::Error::other(format!(
                "installed fixer asset changed: {:?}",
                asset.display()
            )));
        }
        let Some(bytes) = tool.prepare_config(content.as_bytes())? else {
            return Ok(None);
        };
        let _asset = self.inputs.insert(asset, content);
        let _document = self
            .inputs
            .insert(contract.path.clone(), contract.document.clone());
        let location = self
            .checkout
            .path()
            .join(index.to_string())
            .join(tool.find_asset_path());
        let parent = location
            .parent()
            .ok_or_else(|| io::Error::other("missing config directory"))?;
        fs::create_dir_all(parent)?;
        fs::write(&location, bytes)?;
        Ok(Some(location.canonicalize()?))
    }
}
