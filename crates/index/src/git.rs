//! Running `git` and reading its output.

use std::path::Path;
use std::process::Command;

use crate::IndexError;

/// Runs `git -C <root> <args>` and returns stdout. Paths are printed as-is
/// (`core.quotePath=false`), not octal-escaped.
pub(crate) fn run(root: &Path, args: &[&str]) -> Result<String, IndexError> {
    run_ok(root, args, &[0])
}

/// Like [`run`], but any exit code in `ok` counts as success. Some commands
/// use 1 for "nothing matched".
pub(crate) fn run_ok(root: &Path, args: &[&str], ok: &[i32]) -> Result<String, IndexError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "core.quotePath=false"])
        .args(args)
        .output()
        .map_err(IndexError::GitMissing)?;
    if !out.status.code().is_some_and(|c| ok.contains(&c)) {
        return Err(IndexError::Git {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Like [`run`], with extra environment variables for git.
pub(crate) fn run_env(
    root: &Path,
    args: &[&str],
    env: &[(&str, &Path)],
) -> Result<String, IndexError> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .args(["-c", "core.quotePath=false"])
        .args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().map_err(IndexError::GitMissing)?;
    if !out.status.success() {
        return Err(IndexError::Git {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Whether the repo has at least one commit.
pub(crate) fn has_head(root: &Path) -> bool {
    run(root, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok()
}
