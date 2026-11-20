use std::{
    env,
    io,
    path::{Path, PathBuf},
};

use serde_json::{Value, json};
use tokio::process::Command;

use super::{Unresolved, Verification, observation};

#[derive(Clone, Copy)]
pub(super) enum Tool {
    Oxlint,
    Ruff,
}

const OXLINT_CONFIG_KEYS: &[&str] = &["$schema", "rules", "plugins"];

impl Tool {
    pub(super) fn can_check(self, path: &Path) -> bool {
        let extension = path.extension().and_then(|value| value.to_str());
        match self {
            Self::Oxlint => {
                matches!(
                    extension,
                    Some("js" | "jsx" | "ts" | "tsx" | "cjs" | "cts" | "mjs" | "mts")
                )
            },
            Self::Ruff => matches!(extension, Some("py" | "pyi")),
        }
    }

    pub(super) fn find_asset_path(self) -> &'static Path {
        Path::new(match self {
            Self::Oxlint => "assets/oxlint/oxlintrc.json",
            Self::Ruff => "assets/ruff/ruff.toml",
        })
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Oxlint => "oxlint",
            Self::Ruff => "ruff",
        }
    }

    pub(super) fn prepare_config(self, bytes: &[u8]) -> io::Result<Option<Vec<u8>>> {
        if matches!(self, Self::Ruff) {
            return Ok(Some(bytes.to_vec()));
        }
        let mut document: Value = serde_json::from_slice(bytes)?;
        let Some(config) = document.as_object_mut() else {
            return Ok(None);
        };
        // Drop the engine's default rule selection. Only native rules in this asset may fix
        // source. Unsupported options can affect scope or load executable extensions.
        if !config.contains_key("rules")
            || config
                .keys()
                .any(|key| !OXLINT_CONFIG_KEYS.contains(&key.as_str()))
        {
            return Ok(None);
        }
        let _previous = config.insert("categories".into(), json!({"correctness": "off"}));
        Ok(Some(serde_json::to_vec(&document)?))
    }
}

pub(super) struct Job {
    config: PathBuf,
    contract: String,
    files: Vec<PathBuf>,
    tool: Tool,
}

impl Job {
    fn build_command(&self, tree: &Path, fixing: bool) -> Command {
        let mut command = Command::new(self.tool.name());
        let _environment = command.current_dir(tree).env_clear().kill_on_drop(true);
        for key in ["PATH", "SystemRoot", "WINDIR"] {
            if let Some(value) = env::var_os(key) {
                let _value = command.env(key, value);
            }
        }
        match self.tool {
            Tool::Oxlint => {
                let _options = command
                    .args([
                        "--format",
                        "json",
                        "--disable-nested-config",
                        "--deny-warnings",
                        "--config",
                    ])
                    .arg(&self.config);
                if fixing {
                    let _fix = command.arg("--fix");
                }
            },
            Tool::Ruff => {
                let _options = command
                    .args([
                        "check",
                        "--output-format",
                        "json",
                        "--no-cache",
                        "--no-unsafe-fixes",
                        "--no-fix-only",
                        "--config",
                    ])
                    .arg(&self.config)
                    .args(["--config", "lint.extend-safe-fixes=[]"])
                    .arg(if fixing { "--fix" } else { "--no-fix" });
            },
        }
        command
    }

    pub(super) const fn new(
        contract: String,
        tool: Tool,
        config: PathBuf,
        files: Vec<PathBuf>,
    ) -> Self {
        Self {
            config,
            contract,
            files,
            tool,
        }
    }

    pub(super) async fn observe(&self, tree: &Path, fixing: bool) -> io::Result<Verification> {
        let mut finding = None;
        for files in self.files.chunks(32) {
            let mut command = self.build_command(tree, fixing);
            let _files = command.arg("--").args(files);
            let observed = observation::read(command, self.tool.name(), tree, files).await?;
            if finding.is_none() {
                finding = observed;
            }
        }
        let passed = finding.is_none();
        Ok(Verification {
            contract: self.contract.clone(),
            finding,
            passed,
            tool: self.tool.name(),
        })
    }

    pub(super) fn report_unresolved(&self, error: &io::Error) -> Unresolved {
        Unresolved {
            contract: self.contract.clone(),
            message: error.to_string(),
            tool: self.tool.name(),
        }
    }
}
