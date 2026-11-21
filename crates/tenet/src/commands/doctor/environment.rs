use std::{env, path::Path, process::Command};

use ev_grep_jev::Provider;

use super::{CheckStatus, DoctorItem};
use crate::{
    provider::{OPENROUTER_MODEL, TYPESAFE_MODEL},
    report_format::format_display,
};

const MIN_BWRAP_MINOR: u32 = 7;
const MIN_NODE_MAJOR: u32 = 22;

fn read_tool_version(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    Some(format_display(line))
}

fn parse_major_minor(text: &str) -> Option<(u32, u32)> {
    let token = text
        .split_whitespace()
        .find(|part| part.chars().any(|character| character.is_ascii_digit()))?
        .trim_start_matches('v');
    let mut parts = token.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts
        .next()
        .and_then(|value| {
            let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .unwrap_or(0);
    Some((major, minor))
}

pub(super) fn check_credentials() -> DoctorItem {
    let parsed_provider =
        env::var("TENET_PROVIDER").map_or(Ok(Provider::OpenRouter), |raw| raw.parse::<Provider>());
    let provider = match parsed_provider {
        Ok(value) => value,
        Err(error) => {
            return DoctorItem {
                detail: format!("invalid TENET_PROVIDER: {error}"),
                hint: Some("Set TENET_PROVIDER to `openrouter` or `typesafe`.".into()),
                label: "Credentials",
                name: "credentials",
                required: true,
                status: CheckStatus::Fail,
            };
        },
    };
    let key_name = provider.read_key_variable();
    let model = env::var("TENET_MODEL").unwrap_or_else(|_| {
        match provider {
            Provider::OpenRouter => OPENROUTER_MODEL.into(),
            Provider::TypeSafe => TYPESAFE_MODEL.into(),
        }
    });
    match env::var(key_name) {
        Ok(value) if !value.trim().is_empty() => {
            DoctorItem {
                detail: format!("{key_name} set \u{b7} {provider}/{model}"),
                hint: None,
                label: "Credentials",
                name: "credentials",
                required: true,
                status: CheckStatus::Ok,
            }
        },
        Ok(_) | Err(_) => {
            DoctorItem {
                detail: format!("{key_name} is not set"),
                hint: Some(format!(
                    "Export {key_name} before running `tenet check` (provider: {provider})."
                )),
                label: "Credentials",
                name: "credentials",
                required: true,
                status: CheckStatus::Fail,
            }
        },
    }
}

pub(super) fn check_node() -> DoctorItem {
    let Some(node_ver) = read_tool_version("node", &["--version"]) else {
        return DoctorItem {
            detail: "node not found on PATH".into(),
            hint: Some(
                "Install Node.js 22+ to enable generated behavior checks and fx explanations."
                    .into(),
            ),
            label: "Node.js",
            name: "node",
            required: false,
            status: CheckStatus::Warn,
        };
    };
    let npm_ver = read_tool_version("npm", &["--version"]);
    let major = parse_major_minor(&node_ver).map_or(0, |(major_num, _)| major_num);
    let npm_note = npm_ver
        .as_deref()
        .map_or_else(|| "npm missing".into(), |ver| format!("npm {ver}"));
    let ok = major >= MIN_NODE_MAJOR && npm_ver.is_some();
    DoctorItem {
        detail: format!("node {node_ver} \u{b7} {npm_note}"),
        hint: (!ok).then(|| {
            "Upgrade to Node.js 22+ with npm for generated behavior checks and fx.".into()
        }),
        label: "Node.js",
        name: "node",
        required: false,
        status: if ok {
            CheckStatus::Ok
        } else {
            CheckStatus::Warn
        },
    }
}

const MACOS_PREFLIGHT_PROFILE: &str = "(version 1)(deny default)(allow process-exec)(allow \
                                       file-read*)(allow file-map-executable)(allow \
                                       sysctl-read)(deny network*)";

fn probe_macos_sandbox() -> (bool, String, Option<String>) {
    if !Path::new("/usr/bin/sandbox-exec").is_file() {
        return (
            false,
            "/usr/bin/sandbox-exec not found".into(),
            Some("Behavior-check isolation on macOS requires /usr/bin/sandbox-exec.".into()),
        );
    }
    let output = Command::new("/usr/bin/sandbox-exec")
        .args(["-p", MACOS_PREFLIGHT_PROFILE, "/usr/bin/true"])
        .output();
    match output {
        Ok(result) if result.status.success() => {
            (true, "macOS Seatbelt (/usr/bin/sandbox-exec)".into(), None)
        },
        Ok(result)
            if String::from_utf8_lossy(&result.stderr).contains("Operation not permitted") =>
        {
            (
                false,
                "macOS Seatbelt blocked by outer sandbox".into(),
                Some(
                    "macOS sandbox-exec cannot nest inside an outer sandbox (such as an agent \
                     workspace sandbox); run outside the outer macOS sandbox or in Linux with \
                     bubblewrap."
                        .into(),
                ),
            )
        },
        _ => {
            (
                false,
                "macOS Seatbelt preflight failed".into(),
                Some("Ensure /usr/bin/sandbox-exec can run in this environment.".into()),
            )
        },
    }
}

fn check_macos_sandbox() -> DoctorItem {
    let (ready, detail, hint) = probe_macos_sandbox();
    DoctorItem {
        detail,
        hint,
        label: "Sandbox",
        name: "sandbox",
        required: true,
        status: if ready {
            CheckStatus::Ok
        } else {
            CheckStatus::Fail
        },
    }
}

fn check_linux_sandbox() -> DoctorItem {
    let bwrap = read_tool_version("bwrap", &["--version"]);
    let prlimit = read_tool_version("prlimit", &["--version"]);
    let bwrap_ok = bwrap
        .as_deref()
        .and_then(parse_major_minor)
        .is_some_and(|(major, minor)| major > 0 || minor >= MIN_BWRAP_MINOR);
    let ready = bwrap_ok && prlimit.is_some();
    DoctorItem {
        detail: if ready {
            format!(
                "{} \u{b7} prlimit available",
                bwrap.as_deref().unwrap_or("bwrap")
            )
        } else {
            format!(
                "bwrap: {} \u{b7} prlimit: {}",
                bwrap.as_deref().unwrap_or("missing"),
                if prlimit.is_some() { "ok" } else { "missing" }
            )
        },
        hint: (!ready).then(|| {
            "Install bubblewrap (>= 0.7) and util-linux (`prlimit`) for Linux sandbox isolation."
                .into()
        }),
        label: "Sandbox",
        name: "sandbox",
        required: true,
        status: if ready {
            CheckStatus::Ok
        } else {
            CheckStatus::Fail
        },
    }
}

pub(super) fn check_sandbox() -> DoctorItem {
    if cfg!(target_os = "macos") {
        check_macos_sandbox()
    } else if cfg!(target_os = "linux") {
        check_linux_sandbox()
    } else {
        DoctorItem {
            detail: format!(
                "unsupported platform for local sandbox ({})",
                env::consts::OS
            ),
            hint: Some("Generated behavior-check sandboxing supports macOS and Linux.".into()),
            label: "Sandbox",
            name: "sandbox",
            required: false,
            status: CheckStatus::Warn,
        }
    }
}

pub(super) fn check_runtimes() -> DoctorItem {
    let mut found = Vec::new();
    if let Some(python) = read_tool_version("python3", &["--version"]) {
        found.push(python);
    }
    if let Some(rustc) = read_tool_version("rustc", &["--version"]) {
        found.push(rustc);
    }
    if let Some(go_version) = read_tool_version("go", &["version"]) {
        found.push(go_version);
    }
    if found.is_empty() {
        DoctorItem {
            detail: "no python3, rustc, or go found on PATH".into(),
            hint: Some(
                "Install python3, rustc, or go if you want language-specific behavior probes."
                    .into(),
            ),
            label: "Runtimes",
            name: "runtimes",
            required: false,
            status: CheckStatus::Warn,
        }
    } else {
        DoctorItem {
            detail: found.join(" \u{b7} "),
            hint: None,
            label: "Runtimes",
            name: "runtimes",
            required: false,
            status: CheckStatus::Ok,
        }
    }
}
