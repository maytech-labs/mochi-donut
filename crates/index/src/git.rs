//! Running `git` and reading its output.

use std::path::Path;
use std::process::Command;

use crate::IndexError;

/// Runs `git -C <root> <args>` and returns stdout. Paths are printed as-is
/// (`core.quotePath=false`), not octal-escaped.
pub(crate) fn run(root: &Path, args: &[&str]) -> Result<String, IndexError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "core.quotePath=false"])
        .args(args)
        .output()
        .map_err(IndexError::GitMissing)?;
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
