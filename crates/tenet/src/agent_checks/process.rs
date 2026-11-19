use core::time::Duration;
use std::{
    fs,
    io,
    path::Path,
    process::{Child, Stdio},
    thread::sleep,
    time::Instant,
};

use serde_json::Value;

use super::Finding;

pub(super) fn execute(input: &Value, root: &Path, generate: bool) -> io::Result<Vec<Finding>> {
    let workspace = tempfile::tempdir()?;
    for (name, source) in super::FILES {
        fs::write(workspace.path().join(name), source)?;
    }
    // Start the trusted worker outside the project. Node options and project npm settings
    // must not inject code into a process holding the provider credential.
    let request = workspace.path().join("request.json");
    let answer = workspace.path().join("answer.json");
    fs::write(&request, serde_json::to_vec(input)?)?;
    let mut child = super::environment::build_worker(root, generate)?
        .arg(workspace.path().join("worker.mjs"))
        .current_dir(workspace.path())
        .stdin(fs::File::open(request)?)
        .stdout(fs::File::create_new(&answer)?)
        .stderr(Stdio::null())
        .spawn()?;
    wait_bounded(&mut child, &answer)?;
    Ok(serde_json::from_slice(&fs::read(answer)?)?)
}

fn wait_bounded(child: &mut Child, answer: &Path) -> io::Result<()> {
    let start = Instant::now();
    let max_output = 4 * 1024 * 1024;
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                return Err(io::Error::other(format!("Node exited with {status:?}")));
            }
            break;
        }
        if start.elapsed() >= Duration::from_secs(600) || fs::metadata(answer)?.len() > max_output {
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
