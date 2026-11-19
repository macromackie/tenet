use std::path::Path;

use super::{GitError, commit, find_top, run_git};

const GIT_OID_LENGTHS: &[usize] = &[40, 64];

/// Select the pull request base in its CI workspace.
///
/// Other checkouts keep their own scope. A synthetic test merge includes target-branch changes, so
/// compare its tree to its first parent. A head checkout uses the common ancestor to exclude
/// unrelated work on the target branch.
#[inline]
pub fn pull_request_base(
    root: &Path,
    workspace: &Path,
    target: &str,
    head: &str,
) -> Result<Option<String>, GitError> {
    if find_top(root)? != workspace {
        return Ok(None);
    }

    for id in [target, head] {
        if !GIT_OID_LENGTHS.contains(&id.len()) || !id.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(GitError::InvalidPullRequestCommit);
        }
    }
    if commit(root, "HEAD")? == head {
        let bytes = run_git(root, &["merge-base", target, head])?;
        return Ok(Some(String::from_utf8(bytes)?.trim().to_owned()));
    }

    let parents = String::from_utf8(run_git(root, &["show", "-s", "--format=%P", "HEAD"])?)?;
    if parents.split_whitespace().eq([target, head]) {
        return Ok(Some(target.to_owned()));
    }
    Err(GitError::WrongPullRequestCheckout)
}
