use std::path::Path;

use tenet_adoption as adoption;
use tenet_checks::behavior::{find_pin_path, read_pin_summary};
use tenet_engine::{discover_contracts, review::DIRECTORY, run_git};

use super::{CheckStatus, DoctorItem};
use crate::report_format::format_display;

pub(super) fn check_git(root: &Path) -> (bool, DoctorItem) {
    let in_git = run_git(root, &["rev-parse", "--is-inside-work-tree"]).is_ok();
    let item = if in_git {
        DoctorItem {
            detail: "Git repository detected".into(),
            hint: None,
            label: "Git",
            name: "git",
            required: false,
            status: CheckStatus::Ok,
        }
    } else {
        DoctorItem {
            detail: "not inside a Git repository".into(),
            hint: Some("Initialize Git to use diff reviews (`tenet check --base <ref>`).".into()),
            label: "Git",
            name: "git",
            required: false,
            status: CheckStatus::Warn,
        }
    };
    (in_git, item)
}

pub(super) fn check_contracts(root: &Path) -> DoctorItem {
    match discover_contracts(root) {
        Ok(list) if list.is_empty() => {
            DoctorItem {
                detail: "no .contracts/ found yet".into(),
                hint: Some(
                    "Run `tenet init` to adopt recommended contracts or `tenet init --starter`."
                        .into(),
                ),
                label: "Contracts",
                name: "contracts",
                required: false,
                status: CheckStatus::Warn,
            }
        },
        Ok(list) => {
            let noun = if list.len() == 1 {
                "contract"
            } else {
                "contracts"
            };
            DoctorItem {
                detail: format!("{} valid {noun} discovered", list.len()),
                hint: None,
                label: "Contracts",
                name: "contracts",
                required: true,
                status: CheckStatus::Ok,
            }
        },
        Err(error) => {
            DoctorItem {
                detail: format_display(&error.to_string()),
                hint: Some("Fix the contract document and re-run `tenet validate`.".into()),
                label: "Contracts",
                name: "contracts",
                required: true,
                status: CheckStatus::Fail,
            }
        },
    }
}

pub(super) fn check_lockfile(root: &Path) -> DoctorItem {
    match adoption::validate_lock(root) {
        Ok(recorded) if recorded.contracts == 0 && recorded.skills == 0 => {
            DoctorItem {
                detail: "no .tenet.lock (local contracts only)".into(),
                hint: None,
                label: "Lockfile",
                name: "lock",
                required: true,
                status: CheckStatus::Ok,
            }
        },
        Ok(recorded) => {
            DoctorItem {
                detail: format!(
                    "{} locked contract(s) \u{b7} {} locked skill(s)",
                    recorded.contracts, recorded.skills
                ),
                hint: None,
                label: "Lockfile",
                name: "lock",
                required: true,
                status: CheckStatus::Ok,
            }
        },
        Err(error) => {
            DoctorItem {
                detail: format_display(&error.to_string()),
                hint: Some("Check .tenet.lock or run `tenet contracts outdated --offline`.".into()),
                label: "Lockfile",
                name: "lock",
                required: true,
                status: CheckStatus::Fail,
            }
        },
    }
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "Pin and check counts are bounded by the discovered contract documents."
)]
fn count_behavior_pins(
    root: &Path,
    contracts: &[tenet_contracts::Contract],
) -> (usize, usize, usize) {
    let (mut files, mut checks, mut invalid) = (0_usize, 0_usize, 0_usize);
    for contract in contracts {
        let contract_dir = contract
            .path
            .parent()
            .map_or_else(|| root.to_path_buf(), |parent| root.join(parent));
        let Some(pin_path) = find_pin_path(&contract_dir) else {
            continue;
        };
        files += 1;
        match read_pin_summary(&pin_path) {
            Ok(summary) => checks += summary.check_count,
            Err(_) => invalid += 1,
        }
    }
    (files, checks, invalid)
}

pub(super) fn check_behavior_pins(root: &Path) -> DoctorItem {
    let Ok(contracts) = discover_contracts(root) else {
        return DoctorItem {
            detail: "skipped (contracts could not be parsed)".into(),
            hint: None,
            label: "Behavior pins",
            name: "behavior_pins",
            required: false,
            status: CheckStatus::Ok,
        };
    };
    let (files, checks, invalid) = count_behavior_pins(root, &contracts);
    if invalid > 0 {
        return DoctorItem {
            detail: format!("{invalid} of {files} behavior pin file(s) are malformed"),
            hint: Some(
                "Remove or fix the malformed scripts/.lock.json or agent-checks.lock.json file \
                 and rerun `tenet check`."
                    .into(),
            ),
            label: "Behavior pins",
            name: "behavior_pins",
            required: false,
            status: CheckStatus::Warn,
        };
    }
    let detail = if files == 0 {
        "none yet (created automatically on `tenet check`)".into()
    } else {
        format!("{files} pin file(s) \u{b7} {checks} frozen behavior check(s)")
    };
    DoctorItem {
        detail,
        hint: None,
        label: "Behavior pins",
        name: "behavior_pins",
        required: false,
        status: CheckStatus::Ok,
    }
}

pub(super) fn check_state(root: &Path, state: &Path, in_git: bool) -> DoctorItem {
    let tracked = in_git
        && run_git(root, &["ls-files", DIRECTORY])
            .is_ok_and(|files| !files.iter().all(u8::is_ascii_whitespace));
    if tracked {
        return DoctorItem {
            detail: format!("{DIRECTORY}/ contains Git-tracked files"),
            hint: Some(format!(
                "Run `git rm -r --cached {DIRECTORY}` and add `{DIRECTORY}/` to `.gitignore`."
            )),
            label: "Review state",
            name: "state",
            required: true,
            status: CheckStatus::Fail,
        };
    }
    if in_git
        && state == root.join(DIRECTORY)
        && state.is_dir()
        && !state.join(".gitignore").is_file()
        && run_git(root, &["check-ignore", "-q", DIRECTORY]).is_err()
    {
        return DoctorItem {
            detail: format!("{} exists without a .gitignore guard", state.display()),
            hint: Some(format!(
                "Run `tenet check` or write `*` to `{DIRECTORY}/.gitignore` so review state stays \
                 untracked."
            )),
            label: "Review state",
            name: "state",
            required: false,
            status: CheckStatus::Warn,
        };
    }
    let detail = if state.exists() {
        format!("{} present (untracked)", state.display())
    } else {
        format!("{} ready (created on first check)", state.display())
    };
    DoctorItem {
        detail,
        hint: None,
        label: "Review state",
        name: "state",
        required: true,
        status: CheckStatus::Ok,
    }
}
