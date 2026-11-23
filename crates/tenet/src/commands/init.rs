mod starter;

use std::{
    env,
    io::Write,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context as _, anyhow};
use serde_json::json;
use starter::run_starter;
use tenet_adoption::{self as adoption, AddRequest, Added, Catalog, Destination};

use crate::{
    cli::{ExitStatus, Init},
    error::CommandError,
    output,
    report_format::{format_dim, format_display, format_marker, quote_argument},
};

pub(super) const INIT_VERSION: u64 = 1;
pub(in crate::commands) const MAX_INIT_CANDIDATES: usize = 5;

pub(super) fn write_credential_hint(out: &mut impl Write) -> Result<(), CommandError> {
    let provider = env::var("TENET_PROVIDER").unwrap_or_default();
    let has_openrouter = env::var_os("OPENROUTER_API_KEY").is_some_and(|value| !value.is_empty());
    let has_typesafe = env::var_os("TYPESAFE_API_KEY").is_some_and(|value| !value.is_empty());
    let missing = if provider.eq_ignore_ascii_case("typesafe") {
        (!has_typesafe).then_some("TYPESAFE_API_KEY")
    } else {
        (!has_openrouter).then_some("OPENROUTER_API_KEY")
    };
    if let Some(variable) = missing {
        writeln!(
            out,
            "  {} Set `{variable}` in your environment before running `tenet check`.",
            output::format_styled("= help:").cyan(),
        )?;
    }
    Ok(())
}

pub(in crate::commands) fn normalize_scope(
    root: &Path,
    scope: &Path,
) -> Result<PathBuf, CommandError> {
    let mut normalized = PathBuf::new();
    for component in scope.components() {
        match component {
            Component::CurDir => {},
            Component::Normal(segment) => normalized.push(segment),
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err(anyhow!(
                    "--scope must be a directory inside the repository root: {:?}",
                    scope.display()
                )
                .into());
            },
        }
    }
    if !root.join(&normalized).is_dir() {
        return Err(anyhow!("scope directory does not exist: {:?}", scope.display()).into());
    }
    Ok(normalized)
}

/// Install all recommended contracts from `catalog` into `scope`.
pub(in crate::commands) fn install_catalog_candidates(
    root: &Path,
    scope: &Path,
    library: Option<&Path>,
    catalog: &Catalog,
) -> Result<Vec<Added>, CommandError> {
    let (source_alias, sources) = match library {
        Some(directory) => {
            let canonical = directory
                .canonicalize()
                .with_context(|| format!("canonicalize {:?}", directory.display()))?;
            let path_str = canonical
                .to_str()
                .context("catalog path must be valid UTF-8")?;
            ("local", vec![format!("local={path_str}")])
        },
        None => ("tenet", Vec::new()),
    };
    let mut installed = Vec::new();
    for item in &catalog.candidates {
        let spec = format!("{source_alias}/{}@{}", item.contract, catalog.release);
        let added = adoption::add(
            root,
            &AddRequest {
                destination: Destination::Scope {
                    name: None,
                    number: None,
                    scope,
                },
                sources: &sources,
                spec: &spec,
            },
        )?;
        installed.push(added);
    }
    Ok(installed)
}

/// Scaffold a local starter contract, shared by `tenet init --starter` and `tenet scan --starter`.
pub(in crate::commands) fn run_starter_command(
    root: &Path,
    scope: &Path,
    name: &str,
    json: bool,
) -> Result<ExitStatus, CommandError> {
    run_starter(root, scope, name, json)
}

/// Serialize installed catalog contracts for JSON output in `init` and `scan`.
pub(in crate::commands) fn serialize_added(installed: &[Added]) -> Vec<serde_json::Value> {
    installed
        .iter()
        .map(|added| {
            json!({
                "name": added.name,
                "path": added.path,
                "source": added.source,
                "reference": added.reference,
                "commit": added.commit,
            })
        })
        .collect()
}

fn write_installed(
    out: &mut impl Write,
    catalog: &Catalog,
    installed: &[Added],
    json: bool,
) -> Result<(), CommandError> {
    if json {
        let entries = serialize_added(installed);
        output::write_document(
            out,
            INIT_VERSION,
            json!({
                "mode": "catalog",
                "applied": true,
                "release": catalog.release,
                "commit": catalog.commit,
                "installed": entries,
            }),
        )?;
        return Ok(());
    }
    writeln!(
        out,
        "{}  {}\n",
        output::format_styled("Initialized contracts").bold(),
        format_dim(&catalog.release),
    )?;
    for added in installed {
        writeln!(
            out,
            "  {} {}  {}",
            format_marker("PASS"),
            added.name,
            format_dim(&added.path.display().to_string()),
        )?;
    }
    writeln!(
        out,
        "\n  {}  Run `tenet doctor` to verify credentials, then `tenet check`.",
        output::format_styled("Next").bold(),
    )?;
    write_credential_hint(out)?;
    Ok(())
}

fn write_catalog_preview(
    out: &mut impl Write,
    init: &Init,
    catalog: &Catalog,
) -> Result<(), CommandError> {
    if catalog.candidates.is_empty() {
        writeln!(
            out,
            "  {} No shared catalog contracts matched this repository yet.",
            format_marker("REVIEW"),
        )?;
        writeln!(
            out,
            "    {} Run `tenet init --starter` to scaffold a local starter contract.",
            output::format_styled("= help:").cyan(),
        )?;
        return Ok(());
    }
    writeln!(
        out,
        "{}  {}\n",
        output::format_styled("Recommended contracts").bold(),
        format_dim(&catalog.release),
    )?;
    let scope_arg = quote_argument(&init.scope.display().to_string());
    for item in &catalog.candidates {
        writeln!(
            out,
            "  tenet/{}  {}",
            output::format_styled(&item.contract).bold(),
            format_dim(&item.path.display().to_string()),
        )?;
        writeln!(
            out,
            "    {}",
            format_display(&format!("{} {}", item.reason, item.message))
        )?;
        writeln!(
            out,
            "    {}",
            format_dim(&format!(
                "tenet contracts add tenet/{} --scope {scope_arg}",
                item.contract
            ))
        )?;
        writeln!(out)?;
    }
    writeln!(
        out,
        "  {} Run `tenet init --yes` to install these automatically, or `tenet init --starter` \
         for a local contract.",
        output::format_styled("= help:").cyan(),
    )?;
    Ok(())
}

pub(super) fn run(root: &Path, init: &Init) -> Result<ExitStatus, CommandError> {
    if let Some(name) = init.starter.as_deref() {
        return run_starter(root, &init.scope, name, init.json);
    }
    let _scope = normalize_scope(root, &init.scope)?;
    let mut catalog = adoption::find_candidates(root, init.library.as_deref())?;
    catalog.candidates.truncate(MAX_INIT_CANDIDATES);
    let mut out = output::open_stdout().lock();
    if init.yes && !catalog.candidates.is_empty() {
        let installed =
            install_catalog_candidates(root, &init.scope, init.library.as_deref(), &catalog)?;
        write_installed(&mut out, &catalog, &installed, init.json)?;
        return Ok(ExitStatus::Passed);
    }
    if init.json {
        output::write_document(
            &mut out,
            INIT_VERSION,
            json!({
                "mode": "catalog",
                "applied": false,
                "release": catalog.release,
                "commit": catalog.commit,
                "recommendations": catalog.candidates,
            }),
        )?;
        return Ok(ExitStatus::Passed);
    }
    write_catalog_preview(&mut out, init, &catalog)?;
    Ok(ExitStatus::Passed)
}
