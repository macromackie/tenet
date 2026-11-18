use alloc::collections::{BTreeMap, BTreeSet};
use std::{
    env,
    fs,
    io::{self, Read as _},
    path::{Path, PathBuf},
};

use globset::GlobBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tenet_contracts::Contract;

use super::super::inputs::read_hash;
use crate::{MAX_CAPTURE_BYTES, Plan};

const MAX_BINDING_BYTES: u64 = 0x0001_0000;
const MAX_FILES: usize = 4096;
const MAX_TOOL_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Protocol {
    #[default]
    Exit,
    Json,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    pub args: Vec<String>,
    #[serde(default)]
    pub cache: bool,
    pub inputs: Vec<String>,
    pub program: String,
    #[serde(default)]
    pub protocol: Protocol,
    pub timeout_seconds: u64,
    pub version: u8,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct Snapshot {
    pub binding: String,
    pub contract: String,
    pub files: BTreeMap<PathBuf, String>,
    pub program: PathBuf,
    pub tool: String,
}

impl Snapshot {
    #[expect(
        clippy::unnecessary_debug_formatting,
        reason = "Dependency paths in diagnostics escape control characters."
    )]
    pub(super) fn find_difference(&self, current: &Self) -> Option<String> {
        if self.contract != current.contract {
            return Some("contract changed".to_owned());
        }
        if self.binding != current.binding {
            return Some("checker binding changed".to_owned());
        }
        if self.program != current.program || self.tool != current.tool {
            return Some("checker executable changed".to_owned());
        }
        if self.files.keys().ne(current.files.keys()) {
            return Some("discovered input set changed".to_owned());
        }
        self.files.iter().find_map(|(path, digest)| {
            (current.files.get(path) != Some(digest))
                .then(|| format!("input {:?} changed", path.as_os_str()))
        })
    }
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    metadata.is_file()
}

fn resolve_executable(root: &Path, program: &str) -> io::Result<PathBuf> {
    let path = Path::new(program);
    if path.components().count() > 1 || path.is_absolute() {
        let candidate = root.join(path);
        if is_executable(&candidate) {
            return Ok(candidate);
        }
        return Err(io::Error::other(format!(
            "checker {program:?} is missing or cannot execute"
        )));
    }
    let search = env::var_os("PATH").unwrap_or_default();
    for folder in env::split_paths(&search) {
        let candidate = root.join(folder).join(path);
        if is_executable(&candidate) {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("cannot find checker {program:?}"),
    ))
}

impl Binding {
    #[expect(
        clippy::unnecessary_debug_formatting,
        reason = "Dependency paths in diagnostics escape control characters."
    )]
    fn hash_inputs(
        &self,
        plan: &Plan,
        contract: &Contract,
    ) -> io::Result<BTreeMap<PathBuf, String>> {
        let mut paths = BTreeSet::new();
        for pattern in &self.inputs {
            let matcher = GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
                .map_err(io::Error::other)?
                .compile_matcher();
            let mut selected = plan
                .inventory
                .iter()
                .filter(|path| matcher.is_match(path))
                .peekable();
            if selected.peek().is_none() {
                return Err(io::Error::other(format!(
                    "no discovered files match {pattern:?}"
                )));
            }
            paths.extend(selected.take(MAX_FILES + 1));
            if paths.len() > MAX_FILES {
                return Err(io::Error::other("checker inputs exceed 4096 files"));
            }
        }
        let mut files = BTreeMap::new();
        let mut budget = MAX_CAPTURE_BYTES;
        for path in paths {
            let digest = read_hash(&plan.root.join(path), &mut budget).map_err(|error| {
                io::Error::other(format!("input {:?}: {error:#}", path.as_os_str()))
            })?;
            drop(files.insert(path.clone(), digest));
        }
        if !files.keys().any(|path| contract.is_governing(path)) {
            return Err(io::Error::other("checker inputs contain no governed file"));
        }
        Ok(files)
    }

    #[expect(
        clippy::unnecessary_debug_formatting,
        reason = "Paths in checker diagnostics escape control characters."
    )]
    pub(super) fn read(
        plan: &Plan,
        contract: &Contract,
        name: &str,
    ) -> io::Result<(Self, Snapshot)> {
        let relative = contract
            .path
            .with_file_name("checks")
            .join(format!("{name}.json"));
        let path = plan.root.join(&relative);
        let detail = |error: io::Error| {
            io::Error::new(
                error.kind(),
                format!("binding {:?}: {error:#}", relative.as_os_str()),
            )
        };
        if !fs::symlink_metadata(&path).map_err(detail)?.is_file() || path.canonicalize()? != path {
            return Err(io::Error::other("binding must be a regular project file"));
        }
        let mut bytes = Vec::new();
        let _count = fs::File::open(path)?
            .take(MAX_BINDING_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > usize::try_from(MAX_BINDING_BYTES).unwrap_or(usize::MAX) {
            return Err(io::Error::other("checker binding exceeds 64 KiB"));
        }
        let binding: Self = serde_json::from_slice(&bytes)
            .map_err(io::Error::other)
            .map_err(detail)?;
        if binding.version != 1
            || binding.program.is_empty()
            || binding.inputs.is_empty()
            || !(1..=600).contains(&binding.timeout_seconds)
        {
            return Err(detail(io::Error::other(
                "expected version 1, a program, inputs, and timeout_seconds between 1 and 600",
            )));
        }
        let files = binding.hash_inputs(plan, contract)?;
        let program = resolve_executable(&plan.root, &binding.program)?;
        let resolved = program.canonicalize()?;
        let mut tool_budget = MAX_TOOL_BYTES;
        let tool = read_hash(&resolved, &mut tool_budget).map_err(|error| {
            io::Error::other(format!("checker {:?}: {error:#}", program.as_os_str()))
        })?;
        Ok((
            binding,
            Snapshot {
                binding: format!("{:x}", Sha256::digest(&bytes)),
                contract: format!("{:x}", Sha256::digest(contract.document.as_bytes())),
                files,
                program,
                tool,
            },
        ))
    }
}
