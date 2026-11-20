use std::path::Path;

use tenet_engine::Plan;

use crate::{cli::Check, report_format::quote_argument};

/// Preserve the original selection, budget, model and explicit receipt boundary in a rerun.
pub(super) fn build_command(
    check: &Check,
    plan: &Plan,
    state: &Path,
    contract: Option<&str>,
) -> String {
    let mut args = vec!["tenet".into(), "check".into(), "--final".into()];
    args.extend([
        "--root".into(),
        plan.root.display().to_string(),
        "--state-dir".into(),
        state.display().to_string(),
    ]);
    if let Some(name) = contract.or(check.input.contract.as_deref()) {
        args.extend(["--contract".into(), name.into()]);
    }
    if let Some(base) = &plan.base {
        args.extend(["--base".into(), base.clone()]);
    } else {
        args.push("--full".into());
    }
    if let Some(receipt) = &check.evidence {
        args.extend(["--evidence".into(), receipt.display().to_string()]);
    }
    add_model_args(&mut args, check);
    if check.fresh {
        args.push("--fresh".into());
    }
    if plan.context_profile == tenet_engine::ContextProfile::Long {
        args.extend(["--context-profile".into(), "long".into()]);
    }
    for context in &check.input.context {
        args.extend(["--context".into(), context.to_string()]);
    }
    if !plan.selected_paths.is_empty() {
        args.push("--".into());
        args.extend(
            plan.selected_paths
                .iter()
                .map(|path| path.display().to_string()),
        );
    }
    args.iter()
        .map(|arg| quote_argument(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn add_model_args(args: &mut Vec<String>, check: &Check) {
    args.extend([
        "--provider".into(),
        check.run.provider.to_string(),
        "--max-requests".into(),
        check.run.max_requests.to_string(),
        "--min-confidence".into(),
        check.run.min_confidence.to_string(),
        "--contract-batch-size".into(),
        check.run.contract_batch_size.to_string(),
    ]);
    if let Some(model) = &check.run.model {
        args.extend(["--model".into(), model.clone()]);
    }
}
