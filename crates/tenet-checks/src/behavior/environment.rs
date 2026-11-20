use std::{
    env,
    io,
    path::{Path, PathBuf},
    process::Command,
};

const NETWORK: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
];
const HOME_PATHS: &[&str] = &[
    "HOME",
    "TMPDIR",
    "XDG_CACHE_HOME",
    "RUSTUP_HOME",
    "CARGO_HOME",
    "SSL_CERT_FILE",
    "NODE_EXTRA_CA_CERTS",
];
const TOOL_SETTINGS: &[&str] = &["RUSTUP_TOOLCHAIN"];

fn is_outside(root: &Path, path: &Path) -> bool {
    path.is_absolute()
        && path
            .ancestors()
            .find_map(|parent| parent.canonicalize().ok())
            .is_some_and(|resolved| !resolved.starts_with(root))
}

fn is_trusted_bin(root: &Path, folder: &Path) -> bool {
    if !is_outside(root, folder) {
        return false;
    }
    ["node", "npm", "rustc", "git"].iter().all(|name| {
        folder
            .join(name)
            .canonicalize()
            .map_or(true, |resolved| !resolved.starts_with(root))
    })
}

/// Project sockets, shell startup hooks and unrelated credentials stay outside the worker.
pub(super) fn build_worker(checkout: &Path, generate: bool) -> io::Result<Command> {
    let root = checkout.canonicalize()?;
    let path = env::var_os("PATH").ok_or_else(|| io::Error::other("Missing PATH"))?;
    let folders: Vec<PathBuf> = env::split_paths(&path)
        .filter(|folder| is_trusted_bin(&root, folder))
        .collect();
    let safe_path = env::join_paths(&folders).map_err(io::Error::other)?;
    let mut command = Command::new("node");
    let _configured = command
        .env_clear()
        .env("PATH", safe_path)
        .env("NODE_USE_ENV_PROXY", "1")
        .env("TENET_WORKER_SOURCE_ROOT", &root);
    for name in HOME_PATHS {
        if let Some(value) = env::var_os(name) {
            if !is_outside(&root, Path::new(&value)) {
                return Err(io::Error::other(format!(
                    "{name:?} must be outside the checkout"
                )));
            }
            let _set = command.env(name, value);
        }
    }
    for name in TOOL_SETTINGS {
        if let Some(value) = env::var_os(name) {
            let _set = command.env(name, value);
        }
    }
    if generate {
        for name in NETWORK
            .iter()
            .copied()
            .chain(["OPENROUTER_API_KEY", "TENET_AGENT_MODEL"])
        {
            if let Some(value) = env::var_os(name) {
                let _set = command.env(name, value);
            }
        }
    }
    Ok(command)
}
