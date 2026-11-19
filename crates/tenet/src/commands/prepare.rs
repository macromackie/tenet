mod pull_request;

use std::{
    io::Write as _,
    path::{Path, PathBuf},
};

pub(super) use pull_request::resolve_pr_base;
use tenet_engine::{Exclusion, PreparedSnapshot, Selection};

use crate::{
    cli::{Check, Input},
    error::CommandError,
    output,
};

#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
pub(super) fn prepare(
    root: &Path,
    input: &Input,
    base: Option<&str>,
    paths: &[PathBuf],
) -> Result<(tenet_engine::Plan, Selection, Option<PreparedSnapshot>), CommandError> {
    let snapshot = input
        .snapshot
        .as_ref()
        .map(|path| PreparedSnapshot::new(path, input.patch.as_deref()))
        .transpose()?;
    let root = snapshot.as_ref().map_or(root, PreparedSnapshot::read_root);
    let selection = Selection {
        paths: paths.to_owned(),
        base: if input.patch.is_some() {
            Some("HEAD".into())
        } else {
            base.map(str::to_owned)
        },
        contract: input.contract.clone(),
    };
    let plan = tenet_engine::plan(root, &selection)?
        .with_context_profile(input.context_profile.read_profile())
        .with_context(&input.context)?;
    for excluded in &plan.excluded_paths {
        let reason = match excluded.reason {
            Exclusion::NotDiscovered => {
                "discovery found no files there; ignore files and protected names such as .git and \
                 .env exclude files"
            },
            Exclusion::NotGoverned => "no selected contract governs its files",
        };
        writeln!(
            output::open_stderr(),
            "warning: {} is not checked: {reason}",
            excluded.path.display()
        )?;
    }
    Ok((plan, selection, snapshot))
}

/// An inferred PR base assumes the existing rules held. A changed rule needs a new full baseline.
pub(super) fn prepare_check(
    root: &Path,
    check: &Check,
) -> Result<(tenet_engine::Plan, Selection, Option<PreparedSnapshot>), CommandError> {
    let input = &check.input;
    if input.base.is_some() || check.full || input.snapshot.is_some() {
        return prepare(root, input, input.base.as_deref(), &input.paths);
    }
    let base = resolve_pr_base(root)?;
    let prepared = prepare(root, input, base.as_deref(), &input.paths)?;
    let (plan, ..) = &prepared;
    if let Some(commit) = &base
        && let Some(contract) = plan
            .contracts
            .iter()
            .find(|contract| plan.contract_changes.binary_search(&contract.path).is_ok())
    {
        writeln!(
            output::open_stderr(),
            "Checking the full scope because contract {:?} changed since {commit}.",
            contract.name
        )?;
        return prepare(root, input, None, &[]);
    }
    Ok(prepared)
}
