use alloc::collections::BTreeSet;
use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

use tempfile::TempDir;
use tenet_contracts::Contract;

use super::{
    super::{Binding, Observation, Snapshot, process, refresh_binding},
    controls::{Manifest, hash, read_bytes},
};
use crate::{MAX_CAPTURE_BYTES, PreparedSnapshot, git};

const MAX_EXECUTABLE_BYTES: usize = 256 * 1024 * 1024;

pub(super) struct Workspace<'config> {
    before: &'config Snapshot,
    contract: &'config Contract,
    directory: TempDir,
    name: &'config str,
    subjects: BTreeSet<PathBuf>,
}

fn write(source: &Path, destination: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("missing input directory"))?;
    fs::create_dir_all(parent)?;
    fs::write(destination, bytes)?;
    fs::set_permissions(destination, fs::metadata(source)?.permissions())
}

fn copy_input(
    root: &Path,
    directory: &Path,
    path: &Path,
    expected: &str,
    limit: usize,
) -> io::Result<usize> {
    let from = root.join(path);
    let bytes = read_bytes(&from, limit)?;
    if hash(&bytes) != expected {
        return Err(io::Error::other("input changed while staging the checker"));
    }
    write(&from, &directory.join(path), &bytes)?;
    Ok(bytes.len())
}

impl<'config> Workspace<'config> {
    pub(super) fn create(
        root: &Path,
        contract: &'config Contract,
        name: &'config str,
        binding: &Binding,
        snapshot: &'config Snapshot,
        manifest: &Manifest,
    ) -> io::Result<Self> {
        let program = snapshot.program.canonicalize()?;
        let local_program = program.strip_prefix(root).ok();
        let declared_program = Path::new(&binding.program);
        if local_program.is_some()
            && (declared_program.is_absolute() || declared_program.components().count() <= 1)
        {
            return Err(io::Error::other(
                "use a project-relative path such as ./bin/check for an in-project executable",
            ));
        }
        validate_subjects(manifest, contract, snapshot, local_program)?;
        let directory = tempfile::tempdir()?;
        let target = directory.path();
        // A nested contract must never inherit an unrelated manifest from the temp directory.
        fs::create_dir_all(target.join(".contracts"))?;
        let mut remaining = MAX_CAPTURE_BYTES;
        for (path, hash) in &snapshot.files {
            let count = copy_input(root, target, path, hash, remaining)?;
            remaining = remaining.saturating_sub(count);
        }
        let binding_path = contract
            .path
            .with_file_name("checks")
            .join(format!("{name}.json"));
        let _binding_size =
            copy_input(root, target, &binding_path, &snapshot.binding, 0x0001_0000)?;
        let _contract_size = copy_input(
            root,
            target,
            &contract.path,
            &snapshot.contract,
            0x0001_0000,
        )?;
        if let Some(path) = local_program {
            if program != snapshot.program {
                return Err(io::Error::other(
                    "project checker executable must be a regular project file without symlinks",
                ));
            }
            let _program_size =
                copy_input(root, target, path, &snapshot.tool, MAX_EXECUTABLE_BYTES)?;
        }
        Ok(Self {
            before: snapshot,
            contract,
            directory,
            name,
            subjects: manifest.subjects.iter().cloned().collect(),
        })
    }

    pub(super) async fn observe(&self, root: &Path) -> io::Result<Observation> {
        let (binding, checked) = refresh_binding(root, self.contract, self.name)?;
        if checked.tool != self.before.tool
            || checked.contract != self.before.contract
            || checked.binding != self.before.binding
            || self
                .before
                .files
                .iter()
                .filter(|(path, _)| !self.subjects.contains(*path))
                .ne(checked
                    .files
                    .iter()
                    .filter(|(path, _)| !self.subjects.contains(*path)))
        {
            return Err(io::Error::other(
                "control checkout did not reproduce the checker and its declared dependencies",
            ));
        }
        let observation = process::observe_checkout(root, &binding, &checked.program).await?;
        let (_, after) = refresh_binding(root, self.contract, self.name)?;
        if let Some(reason) = checked.find_difference(&after) {
            return Err(io::Error::other(format!(
                "control checkout changed during execution: {reason:#}"
            )));
        }
        Ok(observation)
    }

    pub(super) fn prepare(&self, patch: Option<&[u8]>) -> io::Result<(PreparedSnapshot, String)> {
        let file = patch
            .map(|bytes| -> io::Result<_> {
                let file = tempfile::NamedTempFile::new()?;
                fs::write(file.path(), bytes)?;
                Ok(file)
            })
            .transpose()?;
        let checkout = PreparedSnapshot::new(
            self.directory.path(),
            file.as_ref().map(tempfile::NamedTempFile::path),
        )
        .map_err(io::Error::other)?;
        let root = checkout.read_root();
        if patch.is_some() {
            validate_changes(root, &self.subjects)?;
        }
        let identity = git::run_checkout_git(root, &["write-tree"]).map_err(io::Error::other)?;
        Ok((
            checkout,
            str::from_utf8(&identity)
                .map_err(io::Error::other)?
                .trim()
                .to_owned(),
        ))
    }
}

fn validate_changes(root: &Path, subjects: &BTreeSet<PathBuf>) -> io::Result<()> {
    let names = git::run_checkout_git(
        root,
        &[
            "diff",
            "--cached",
            "--no-renames",
            "--name-only",
            "-z",
            "HEAD",
            "--",
        ],
    )
    .map_err(io::Error::other)?;
    if names.is_empty() {
        return Err(io::Error::other("control patch made no changes"));
    }
    for bytes in names
        .split(|value| *value == 0)
        .filter(|part| !part.is_empty())
    {
        let name = str::from_utf8(bytes).map_err(io::Error::other)?;
        if !subjects.contains(Path::new(name)) {
            return Err(io::Error::other(format!(
                "control changed undeclared subject {name:?}"
            )));
        }
        if let Ok(metadata) = fs::symlink_metadata(root.join(name))
            && !metadata.is_file()
        {
            return Err(io::Error::other(format!(
                "control subject {name:?} must remain a regular file"
            )));
        }
    }
    Ok(())
}

#[expect(
    clippy::unnecessary_debug_formatting,
    reason = "Project paths in diagnostics escape control characters."
)]
fn validate_subjects(
    manifest: &Manifest,
    contract: &Contract,
    snapshot: &Snapshot,
    local_program: Option<&Path>,
) -> io::Result<()> {
    for subject in &manifest.subjects {
        if !snapshot.files.contains_key(subject)
            || !contract.is_governing(subject)
            || local_program == Some(subject)
        {
            return Err(io::Error::other(format!(
                "subject {:?} must be a governed checker input and distinct from its executable",
                subject.as_os_str()
            )));
        }
    }
    Ok(())
}
