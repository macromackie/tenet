//! Records the Git commit that the binary is built from, which the first record of a trace names. A
//! build outside a Git checkout records `unknown`.

use std::process::Command;

fn main() {
    let git = |arguments: &[&str]| {
        Command::new("git")
            .args(arguments)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|text| text.trim().to_owned())
    };
    // A commit or checkout rewrites HEAD or its reference log, so the build reads the revision
    // again.
    for file in ["HEAD", "logs/HEAD"] {
        if let Some(path) = git(&["rev-parse", "--git-path", file]) {
            println!("cargo::rerun-if-changed={path}");
        }
    }
    let revision = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_owned());
    println!("cargo::rustc-env=TENET_REVISION={revision}");
}
