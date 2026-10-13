//! Talking to the forge through `gh`, and to the local repo through `git`.
//! The repo is `host/owner/repo`: `--repo`, else the push remote's
//! (docs/taro/design.md, "Forges").

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use mochi_common::proc::run;
use serde_json::Value;

/// A repo on a forge, and the local clone it was found from.
#[derive(Debug, Clone)]
pub struct Repo {
    pub host: String,
    pub owner: String,
    pub name: String,
    /// The local clone's root, when there is one.
    pub root: Option<PathBuf>,
}

impl Repo {
    /// `host/owner/repo`, for `gh -R`.
    pub fn full(&self) -> String {
        format!("{}/{}/{}", self.host, self.owner, self.name)
    }

    /// Finds the repo: `spec` (`host/owner/repo` or `owner/repo`, which
    /// means github.com), else the local clone's push remote.
    pub fn find(spec: Option<&str>, cwd: &Path) -> Result<Repo, String> {
        let root = git(cwd, &["rev-parse", "--show-toplevel"])
            .ok()
            .map(|r| PathBuf::from(r.trim()));
        let full = match spec {
            Some(s) => s.to_owned(),
            None => {
                let Some(root) = &root else {
                    return Err("Not in a git repo: run taro inside a clone, or pass --repo host/owner/repo.".into());
                };
                let remote = push_remote(root)
                    .ok_or("This repo has no remote. Pass --repo host/owner/repo.")?;
                let url = git(root, &["remote", "get-url", "--push", &remote])?;
                normalize_remote(url.trim())
            }
        };
        let parts: Vec<&str> = full.split('/').collect();
        let (host, owner, name) = match parts.as_slice() {
            [owner, name] => ("github.com", *owner, *name),
            [host, owner, name] => (*host, *owner, *name),
            _ => return Err(format!("{full:?} isn't host/owner/repo")),
        };
        if host.contains("gitlab") {
            return Err(format!(
                "{host} is GitLab, which taro doesn't support yet; only GitHub and GitHub Enterprise. `yuzu wait cmd` and `glab` cover GitLab meanwhile."
            ));
        }
        Ok(Repo {
            host: host.to_owned(),
            owner: owner.to_owned(),
            name: name.to_owned(),
            root,
        })
    }
}

/// The remote a push goes to: the branch's remote, else `origin`, else the
/// first one.
fn push_remote(root: &Path) -> Option<String> {
    let names = git(root, &["remote"]).ok()?;
    let branch = git(root, &["symbolic-ref", "--short", "-q", "HEAD"]).ok();
    let configured = branch.and_then(|b| {
        git(
            root,
            &["config", "--get", &format!("branch.{}.remote", b.trim())],
        )
        .ok()
    });
    configured
        .map(|c| c.trim().to_owned())
        .filter(|c| names.lines().any(|n| n == c))
        .or_else(|| names.lines().find(|n| *n == "origin").map(str::to_owned))
        .or_else(|| names.lines().next().map(str::to_owned))
}

/// `git@github.com:me/r.git` and `https://github.com/me/r` as `github.com/me/r`.
pub fn normalize_remote(url: &str) -> String {
    let mut s = url.trim();
    if let Some((_, rest)) = s.split_once("://") {
        s = rest;
    } else if let Some((host, path)) = s.split_once(':')
        && !host.contains('/')
    {
        let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
        return finish(&format!("{host}/{}", path.trim_start_matches('/')));
    }
    let (host, path) = s.split_once('/').unwrap_or((s, ""));
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next().unwrap_or(host);
    finish(&format!("{host}/{path}"))
}

fn finish(s: &str) -> String {
    let s = s.trim_end_matches('/');
    s.strip_suffix(".git").unwrap_or(s).to_owned()
}

/// Runs git in `dir`; stdout, or the first line of stderr.
pub fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args(["-c", "core.quotePath=false"])
        .args(args);
    let out = run(cmd, Duration::from_secs(60))?;
    if out.code == Some(0) {
        Ok(out.stdout)
    } else {
        Err(format!(
            "git {}: {}",
            args.join(" "),
            out.stderr.trim().lines().next().unwrap_or("failed")
        ))
    }
}

/// Runs `gh` and parses its JSON. Auth failures say how to see which login
/// or token gh is using, and name `GH_TOKEN` when it's set, since it
/// overrides every stored login.
pub fn gh(args: &[&str]) -> Result<Value, String> {
    let mut cmd = Command::new("gh");
    cmd.args(args);
    let out = run(cmd, Duration::from_secs(120))
        .map_err(|e| format!("{e}. Is gh installed? https://cli.github.com"))?;
    if out.code != Some(0) {
        let err = out
            .stderr
            .trim()
            .lines()
            .next()
            .unwrap_or("gh failed")
            .to_owned();
        if err.contains("401") || err.contains("Bad credentials") || err.contains("gh auth login") {
            let token = if std::env::var_os("GH_TOKEN").is_some() {
                " GH_TOKEN is set, and for github.com gh uses it over any stored login: unset it or replace it."
            } else {
                ""
            };
            return Err(format!(
                "gh can't authenticate ({err}).{token} `yuzu whoami --verify` shows which login or token it's using."
            ));
        }
        return Err(format!("gh {}: {err}", args.first().copied().unwrap_or("")));
    }
    let text = out.stdout.trim();
    if text.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(text).map_err(|e| {
        format!(
            "gh {} printed something that isn't JSON: {e}",
            args.join(" ")
        )
    })
}

/// `gh api <path>` on the repo's host, or `None` when the forge says it's
/// not there or not allowed (403, 404): a plan feature, or no admin rights.
pub fn api_optional(repo: &Repo, path: &str) -> Result<Option<Value>, String> {
    match gh(&["api", "--hostname", &repo.host, path]) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.contains("403") || e.contains("404") || e.contains("Not Found") => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn api(repo: &Repo, path: &str) -> Result<Value, String> {
    gh(&["api", "--hostname", &repo.host, path])
}

/// A GraphQL query with `owner`, `name`, and `number` variables.
pub fn graphql(repo: &Repo, query: &str, number: u64) -> Result<Value, String> {
    gh(&[
        "api",
        "graphql",
        "--hostname",
        &repo.host,
        "-f",
        &format!("query={query}"),
        // -f keeps them strings; -F would make an owner named 123 a number.
        "-f",
        &format!("owner={}", repo.owner),
        "-f",
        &format!("name={}", repo.name),
        "-F",
        &format!("number={number}"),
    ])
}

/// The PR number: `given`, else the open PR for the current branch.
pub fn pr_number(repo: &Repo, given: Option<u64>) -> Result<u64, String> {
    if let Some(n) = given {
        return Ok(n);
    }
    let root = repo
        .root
        .as_ref()
        .ok_or("Pass a PR number: there's no local branch to find it from.")?;
    let branch = git(root, &["symbolic-ref", "--short", "-q", "HEAD"])
        .map_err(|_| "HEAD is detached: pass a PR number.".to_owned())?;
    let branch = branch.trim();
    let v = gh(&["pr", "view", branch, "-R", &repo.full(), "--json", "number"]).map_err(|e| {
        if e.contains("can't authenticate") {
            e
        } else {
            format!(
                "No open PR for branch {branch} ({e}). Push it and open one, or pass a PR number."
            )
        }
    })?;
    v["number"]
        .as_u64()
        .ok_or_else(|| format!("No PR found for branch {branch}. Pass a PR number."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_remotes() {
        assert_eq!(
            normalize_remote("git@github.com:me/r.git"),
            "github.com/me/r"
        );
        assert_eq!(
            normalize_remote("https://github.com/me/r"),
            "github.com/me/r"
        );
        assert_eq!(
            normalize_remote("ssh://git@ghe.corp:22/team/app.git"),
            "ghe.corp/team/app"
        );
    }

    #[test]
    fn reads_repo_specs_and_refuses_gitlab() {
        let here = std::env::temp_dir();
        let r = Repo::find(Some("trammel-may/mochi-donut"), &here).unwrap();
        assert_eq!(r.full(), "github.com/trammel-may/mochi-donut");
        let r = Repo::find(Some("ghe.corp/team/app"), &here).unwrap();
        assert_eq!((r.host.as_str(), r.owner.as_str()), ("ghe.corp", "team"));
        assert!(
            Repo::find(Some("gitlab.com/a/b"), &here)
                .unwrap_err()
                .contains("GitLab")
        );
        assert!(
            Repo::find(Some("just-one"), &here)
                .unwrap_err()
                .contains("host/owner/repo")
        );
    }
}
