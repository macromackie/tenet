use std::{env, fs::File, io::BufReader, path::Path};

use anyhow::Context as _;
use serde::Deserialize;

use crate::error::CommandError;

#[derive(Deserialize)]
struct Event {
    pull_request: PullRequest,
}

#[derive(Deserialize)]
struct PullRequest {
    base: Revision,
    head: Revision,
}

#[derive(Deserialize)]
struct Revision {
    sha: String,
}

/// A `GitHub` pull request identifies the target commit independently of branch names.
pub(in crate::commands) fn resolve_pr_base(root: &Path) -> Result<Option<String>, CommandError> {
    if env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || !matches!(
            env::var("GITHUB_EVENT_NAME").as_deref(),
            Ok("pull_request" | "pull_request_target")
        )
    {
        return Ok(None);
    }

    let directory =
        env::var_os("GITHUB_WORKSPACE").context("missing GitHub pull request workspace")?;
    let workspace = Path::new(&directory).canonicalize()?;
    if !root.canonicalize()?.starts_with(&workspace) {
        return Ok(None);
    }

    let path =
        env::var_os("GITHUB_EVENT_PATH").context("missing GitHub pull request event path")?;
    let file = File::open(&path).context("open GitHub pull request event")?;
    let event: Event = serde_json::from_reader(BufReader::new(file))
        .context("read pull request base and head from GitHub event")?;
    tenet_engine::pull_request_base(
        root,
        &workspace,
        &event.pull_request.base.sha,
        &event.pull_request.head.sha,
    )
    .context(
        "select the pull request base (check out its head with fetch-depth: 0, or specify --base \
         or --full)",
    )
    .map_err(Into::into)
}
