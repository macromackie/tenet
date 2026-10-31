use std::{io::Write as _, path::Path};

use tenet_engine::{Exclusion, PreparedSnapshot, Selection};

use crate::{cli::Input, error::CommandError, output};

#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
pub(super) fn prepare(
    root: &Path,
    input: &Input,
) -> Result<(tenet_engine::Plan, Selection, Option<PreparedSnapshot>), CommandError> {
    let snapshot = input
        .snapshot
        .as_ref()
        .map(|base| PreparedSnapshot::new(base, input.patch.as_deref()))
        .transpose()?;
    let root = snapshot.as_ref().map_or(root, PreparedSnapshot::read_root);
    let selection = Selection {
        paths: input.paths.clone(),
        base: if input.patch.is_some() {
            Some("HEAD".into())
        } else {
            input.base.clone()
        },
        contract: input.contract.clone(),
    };
    let plan = tenet_engine::plan(root, &selection)?.with_context(&input.context)?;
    plan.admit_context()?;
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
