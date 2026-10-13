//! `taro cleanup`: which branches are merged, proved, whatever the merge
//! method. Local git only, until `--delete` pushes (docs/taro/design.md,
//! "taro cleanup").

use std::path::Path;

use serde::Serialize;

use crate::forge::git;

/// How a branch's content was shown to be in the base.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Proof {
    /// The branch is an ancestor of the base: merge commits, fast-forwards.
    Ancestor,
    /// Every commit's patch is in the base: rebase merges, cherry-picks.
    EveryPatch,
    /// The whole branch's diff is one commit in the base: squash merges.
    Squashed,
    /// Every file the branch changed has the same content in the base.
    SameContent,
}

#[derive(Debug, Clone, Serialize)]
pub struct Branch {
    /// `feat/x`, or `origin/feat/x` for a remote branch.
    pub name: String,
    pub remote: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<Proof>,
    /// Why it isn't safe, when it isn't.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsafe_because: Option<String>,
}

/// Every local branch (and with `remote`, that remote's branches) but the
/// base and the checked-out one, each proved merged or not.
pub fn branches(root: &Path, base: &str, remote: Option<&str>) -> Result<Vec<Branch>, String> {
    let current = git(root, &["symbolic-ref", "--short", "-q", "HEAD"]).unwrap_or_default();
    let current = current.trim();
    let base_name = base.rsplit('/').next().unwrap_or(base);
    let worktrees = worktree_branches(root)?;
    let mut out = Vec::new();
    for name in git(
        root,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )?
    .lines()
    {
        if name.is_empty() || name == base_name || name == base || name == current {
            continue;
        }
        // Work may be in progress there, and git won't delete it anyway.
        if let Some((_, path)) = worktrees.iter().find(|(b, _)| b == name) {
            out.push(Branch {
                name: name.to_owned(),
                remote: None,
                proof: None,
                unsafe_because: Some(format!("it's checked out in the worktree at {path}")),
            });
            continue;
        }
        out.push(judge(root, base, name, None)?);
    }
    if let Some(r) = remote {
        let prefix = format!("refs/remotes/{r}");
        for name in git(
            root,
            &["for-each-ref", "--format=%(refname:short)", &prefix],
        )?
        .lines()
        {
            let short = name.strip_prefix(&format!("{r}/")).unwrap_or(name);
            if name.is_empty() || short == "HEAD" || short == base_name || name == base || name == r
            {
                continue;
            }
            out.push(judge(root, base, name, Some(r))?);
        }
    }
    Ok(out)
}

/// Branches checked out in a worktree, with the worktree's path.
fn worktree_branches(root: &Path) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    let mut path = String::new();
    for line in git(root, &["worktree", "list", "--porcelain"])?.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            p.clone_into(&mut path);
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            out.push((b.to_owned(), path.clone()));
        }
    }
    Ok(out)
}

fn judge(root: &Path, base: &str, branch: &str, remote: Option<&str>) -> Result<Branch, String> {
    let (proof, why) = match prove(root, base, branch)? {
        Ok(p) => (Some(p), None),
        Err(why) => (None, Some(why)),
    };
    Ok(Branch {
        name: branch.to_owned(),
        remote: remote.map(str::to_owned),
        proof,
        unsafe_because: why,
    })
}

/// Shows `branch`'s content is in `base`, or says why not. The outer error
/// is git failing; the inner one is "not merged", with the reason.
pub fn prove(root: &Path, base: &str, branch: &str) -> Result<Result<Proof, String>, String> {
    if git(root, &["merge-base", "--is-ancestor", branch, base]).is_ok() {
        return Ok(Ok(Proof::Ancestor));
    }
    let cherry = git(root, &["cherry", base, branch])?;
    let missing: Vec<&str> = cherry.lines().filter(|l| l.starts_with('+')).collect();
    if !cherry.trim().is_empty() && missing.is_empty() {
        return Ok(Ok(Proof::EveryPatch));
    }
    let fork = git(root, &["merge-base", base, branch])?;
    let fork = fork.trim();
    let whole = patch_id(root, &["diff", fork, branch])?;
    if let Some(id) = &whole {
        let range = format!("{fork}..{base}");
        for c in git(root, &["rev-list", "--no-merges", &range])?
            .lines()
            .take(500)
        {
            if patch_id(root, &["show", c])?.as_ref() == Some(id) {
                return Ok(Ok(Proof::Squashed));
            }
        }
    }
    // Without renames, a file the branch moved counts as deleted too, and
    // must be gone from the base as well.
    let changed = git(root, &["diff", "--name-only", "--no-renames", fork, branch])?;
    let files: Vec<&str> = changed.lines().filter(|l| !l.is_empty()).collect();
    if !files.is_empty() {
        let mut args = vec!["diff", "--quiet", branch, base, "--"];
        args.extend(files.iter());
        if git(root, &args).is_ok() {
            return Ok(Ok(Proof::SameContent));
        }
    }
    let first = missing
        .first()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|sha| git(root, &["log", "-1", "--format=%s", sha]).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();
    let n = missing.len().max(1);
    Ok(Err(if first.is_empty() {
        format!("its changes aren't in {base}")
    } else {
        format!(
            "{n} {} not in {base}, such as \"{first}\"",
            if n == 1 { "commit" } else { "commits" }
        )
    }))
}

/// The stable patch id of `git <args>`'s output, or `None` for an empty diff.
fn patch_id(root: &Path, args: &[&str]) -> Result<Option<String>, String> {
    let diff = git(root, args)?;
    if diff.trim().is_empty() {
        return Ok(None);
    }
    let mut child = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["patch-id", "--stable"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("couldn't run git patch-id: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(diff.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned))
}
