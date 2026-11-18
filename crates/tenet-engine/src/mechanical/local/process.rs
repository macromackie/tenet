use core::time::Duration;
use std::{
    io,
    path::Path,
    process::{ExitStatus, Stdio},
};

#[cfg(unix)]
use rustix::process::{Pid, Signal, kill_process_group};
use tokio::{
    io::{AsyncRead, AsyncReadExt as _},
    process::Command,
    time::timeout,
};

use super::{Binding, Observation, Protocol};
use crate::mechanical::Outcome;

const MAX_STREAM_BYTES: usize = 0x4000;

async fn capture(stream: impl AsyncRead + Unpin) -> io::Result<String> {
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_STREAM_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let _count = stream.take(limit).read_to_end(&mut bytes).await?;
    if bytes.len() > MAX_STREAM_BYTES {
        return Err(io::Error::other(
            "checker output exceeded 16 KiB per stream",
        ));
    }
    Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
}

#[cfg(unix)]
struct ProcessGroup(Pid);

#[cfg(unix)]
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        let _result = kill_process_group(self.0, Signal::KILL);
    }
}

pub(super) async fn observe(
    root: &Path,
    binding: &Binding,
    program: &Path,
) -> io::Result<Observation> {
    let mut command = Command::new(program);
    let _command = command
        .args(&binding.args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    let _group = command.process_group(0);
    let mut child = command.spawn()?;
    #[cfg(unix)]
    let _cleanup = child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(Pid::from_raw)
        .map(ProcessGroup);
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("missing checker stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("missing checker stderr"))?;
    let run = async { tokio::try_join!(child.wait(), capture(stdout), capture(stderr)) };
    let (status, output, errors) = timeout(Duration::from_secs(binding.timeout_seconds), run)
        .await
        .map_err(|error| {
            io::Error::other(format!(
                "checker timed out after {:?} seconds: {error:#}",
                binding.timeout_seconds
            ))
        })??;
    decode_output(binding.protocol, status, output, errors)
}

fn decode_output(
    protocol: Protocol,
    status: ExitStatus,
    output: String,
    errors: String,
) -> io::Result<Observation> {
    if matches!(protocol, Protocol::Json) && status.success() {
        let result: Observation = serde_json::from_str(&output).map_err(io::Error::other)?;
        if result.message.trim().is_empty() {
            return Err(io::Error::other(
                "structured checker returned an empty message",
            ));
        }
        return Ok(result);
    }
    let outcome = match (protocol, status.code()) {
        (Protocol::Exit, Some(0_i32)) => Outcome::Pass,
        (Protocol::Exit, Some(1_i32)) => Outcome::Fail,
        _ => Outcome::Unresolved,
    };
    let mut message = status.to_string();
    for stream in [output, errors] {
        if !stream.is_empty() {
            message.push('\n');
            message.push_str(&stream);
        }
    }
    Ok(Observation {
        finding: None,
        message,
        outcome,
    })
}
