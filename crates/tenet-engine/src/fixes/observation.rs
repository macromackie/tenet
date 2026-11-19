use core::time::Duration;
use std::{
    io,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
};

use serde_json::Value;
use tokio::{
    io::{AsyncRead, AsyncReadExt as _},
    process::Command,
    time::timeout,
};

async fn capture(reader: impl AsyncRead + Unpin) -> io::Result<Vec<u8>> {
    const LIMIT: usize = 512 * 1024;
    let mut bytes = Vec::new();
    let size = reader
        .take(u64::try_from(LIMIT).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)
        .await?;
    if size > LIMIT {
        return Err(io::Error::other(
            "native lint output exceeds 512 KiB; narrow the selected paths",
        ));
    }
    Ok(bytes)
}

pub(super) async fn read(
    mut command: Command,
    tool: &str,
    tree: &Path,
    files: &[PathBuf],
) -> io::Result<Option<String>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| io::Error::new(error.kind(), format!("cannot run {tool:?}: {error:#}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("missing native lint output"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("missing native lint diagnostics"))?;
    let (status, output, errors) = timeout(Duration::from_secs(60), async {
        tokio::try_join!(child.wait(), capture(stdout), capture(stderr))
    })
    .await
    .map_err(|error| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!("{tool:?} exceeded 60 seconds: {error:#}"),
        )
    })??;
    let payload: Value = serde_json::from_slice(&output).map_err(|source| {
        let diagnostic: String = String::from_utf8_lossy(&errors)
            .chars()
            .filter(|character| !character.is_control())
            .take(400)
            .collect();
        io::Error::other(format!(
            "{tool:?} returned {status:?} without readable findings ({diagnostic:?}): {source:#}"
        ))
    })?;
    decode(&payload, status, tool, tree, files)
}

fn decode(
    payload: &Value,
    status: ExitStatus,
    tool: &str,
    tree: &Path,
    files: &[PathBuf],
) -> io::Result<Option<String>> {
    let diagnostics = if tool == "ruff" {
        payload.as_array()
    } else {
        if payload.get("number_of_files").and_then(Value::as_u64) != u64::try_from(files.len()).ok()
        {
            return Err(io::Error::other(
                "Oxlint did not inspect every selected file",
            ));
        }
        payload.get("diagnostics").and_then(Value::as_array)
    }
    .ok_or_else(|| io::Error::other(format!("{tool:?} returned no diagnostic list")))?;
    match (status.code(), diagnostics.first()) {
        (Some(0_i32), None) => Ok(None),
        (Some(1_i32), Some(first)) => Ok(Some(format_finding(first, tool, tree, files)?)),
        _ => {
            Err(io::Error::other(format!(
                "{tool:?} exit and findings disagree ({status:?})"
            )))
        },
    }
}

fn format_finding(first: &Value, tool: &str, tree: &Path, files: &[PathBuf]) -> io::Result<String> {
    let message = first
        .get("message")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("missing lint finding message"))?;
    let filename = first
        .get("filename")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("missing lint finding path"))?;
    let file = Path::new(filename);
    let relative = file.strip_prefix(tree).unwrap_or(file);
    if !files.iter().any(|target| target == relative) {
        return Err(io::Error::other(format!(
            "{tool:?} returned a finding outside the selected files"
        )));
    }
    let line = first
        .pointer("/location/row")
        .or_else(|| first.pointer("/labels/0/span/line"))
        .and_then(Value::as_u64);
    let location = line.map_or_else(
        || relative.display().to_string(),
        |number| format!("{}:{number}", relative.display()),
    );
    let text: String = message
        .chars()
        .filter(|character| !character.is_control())
        .take(400)
        .collect();
    Ok(format!("{location}  {text}"))
}
