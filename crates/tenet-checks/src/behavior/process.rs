use core::time::Duration;
use std::{
    fs,
    io,
    path::Path,
    process::{Child, ExitStatus},
    thread::sleep,
    time::Instant,
};

use serde_json::Value;

use super::Finding;

const MAX_STDERR_BYTES: usize = 4_096;
const PROGRESS_TICK: Duration = Duration::from_millis(80);
const WHITESPACE_CONTROLS: &[char] = &['\n', '\r', '\t'];

fn clean_phase_text(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter_map(|character| {
            if WHITESPACE_CONTROLS.contains(&character) {
                Some(' ')
            } else if character.is_control() {
                None
            } else {
                Some(character)
            }
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn execute(
    input: &Value,
    root: &Path,
    generate: bool,
    on_progress: Option<&mut dyn FnMut(&str)>,
) -> io::Result<Vec<Finding>> {
    let bytes = execute_bytes(input, root, generate, on_progress)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub(super) fn execute_bytes(
    input: &Value,
    root: &Path,
    generate: bool,
    on_progress: Option<&mut dyn FnMut(&str)>,
) -> io::Result<Vec<u8>> {
    let workspace = tempfile::tempdir()?;
    for (name, source) in super::FILES {
        fs::write(workspace.path().join(name), source)?;
    }
    // Start the trusted worker outside the project. Node options and project npm settings
    // must not inject code into a process holding the provider credential.
    let request = workspace.path().join("request.json");
    let answer = workspace.path().join("answer.json");
    let errors = workspace.path().join("stderr.log");
    let progress = workspace.path().join("progress.txt");
    let mut payload = input.clone();
    if let Some(fields) = payload.as_object_mut() {
        let _prior = fields.insert("progressFile".into(), serde_json::json!(progress));
    }
    fs::write(&request, serde_json::to_vec(&payload)?)?;
    let mut child = super::environment::build_worker(root, generate)?
        .arg(workspace.path().join("worker.mjs"))
        .current_dir(workspace.path())
        .stdin(fs::File::open(request)?)
        .stdout(fs::File::create_new(&answer)?)
        .stderr(fs::File::create_new(&errors)?)
        .spawn()?;
    wait_bounded(&mut child, &answer, &errors, &progress, on_progress)?;
    fs::read(answer)
}

fn read_progress(progress: &Path) -> Option<String> {
    let text = fs::read_to_string(progress).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let label = trimmed.split_once('\t').map_or_else(
        || clean_phase_text(trimmed),
        |(contract, phase)| {
            format!(
                "{} \u{b7} {}",
                clean_phase_text(contract),
                clean_phase_text(phase)
            )
        },
    );
    (!label.is_empty()).then_some(label)
}

fn read_diagnostic(errors: &Path) -> String {
    let Ok(bytes) = fs::read(errors) else {
        return String::new();
    };
    let slice = bytes.get(..MAX_STDERR_BYTES).unwrap_or(&bytes);
    let text = String::from_utf8_lossy(slice);
    text.trim()
        .chars()
        .map(|character| {
            if WHITESPACE_CONTROLS.contains(&character) {
                ' '
            } else {
                character
            }
        })
        .filter(|character| !character.is_control())
        .take(300)
        .collect()
}

fn format_exit_error(status: ExitStatus, errors: &Path) -> io::Error {
    let diagnostic = read_diagnostic(errors);
    let message = if diagnostic.is_empty() {
        format!("Node exited with {status:?}")
    } else {
        format!("Node exited with {status:?}: {diagnostic}")
    };
    io::Error::other(message)
}

fn wait_bounded(
    child: &mut Child,
    answer: &Path,
    errors: &Path,
    progress: &Path,
    mut on_progress: Option<&mut dyn FnMut(&str)>,
) -> io::Result<()> {
    let start = Instant::now();
    let max_output = 4 * 1024 * 1024;
    let mut last_label = String::from("behavior checks");
    let mut last_tick = Instant::now();
    if let Some(callback) = on_progress.as_deref_mut() {
        callback(&last_label);
    }
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                return Err(format_exit_error(status, errors));
            }
            break;
        }
        if let Some(callback) = on_progress.as_deref_mut() {
            let latest = read_progress(progress);
            let changed = latest.as_ref().is_some_and(|label| label != &last_label);
            if let Some(label) = latest {
                last_label = label;
            }
            if changed || last_tick.elapsed() >= PROGRESS_TICK {
                callback(&last_label);
                last_tick = Instant::now();
            }
        }
        if start.elapsed() >= Duration::from_secs(600)
            || fs::metadata(answer)?.len() > max_output
            || fs::metadata(errors)?.len() > max_output
        {
            let _killed = child.kill();
            let _reaped = child.wait();
            return Err(io::Error::other(
                "The checker worker exceeded its time or output limit.",
            ));
        }
        sleep(Duration::from_millis(50));
    }
    if fs::metadata(answer)?.len() > max_output {
        return Err(io::Error::other(
            "The checker result exceeded its output limit.",
        ));
    }
    Ok(())
}
