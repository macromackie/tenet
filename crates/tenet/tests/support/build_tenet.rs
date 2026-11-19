use std::{env, process::Command};

/// Tenet without the caller's trace settings, so a fixture's output never depends on them.
pub(super) fn build_tenet() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tenet"));
    for (name, _) in env::vars_os() {
        if name.to_string_lossy().starts_with("OTEL_") {
            let _: &mut Command = command.env_remove(name);
        }
    }
    let _: &mut Command = command
        .env_remove("TENET_TRACE")
        .env_remove("TENET_TRACE_ENVIRONMENT")
        .env_remove("TRACEPARENT");
    command
}
