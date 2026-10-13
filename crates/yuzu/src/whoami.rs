//! `yuzu whoami`: which account each tool would act as here, where that
//! comes from, and what overrides it. Offline by default; `--verify` asks
//! each service (docs/decisions/0022-yuzu-whoami.md).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Serialize;

use crate::env::Ctx;
use crate::targets::{git_out, normalize_remote, remote_url};
use mochi_common::proc::run;

#[derive(Debug, Clone, Serialize)]
pub struct Identity {
    /// `github`, `git`, `ssh`, or `aws`.
    pub service: &'static str,
    /// The host, remote, or purpose it applies to.
    pub scope: String,
    /// The account, author, key, or profile.
    pub who: String,
    /// Where `who` comes from.
    pub source: String,
    /// Something that makes this identity not what it looks like.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// With `--verify`: what the service says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<Verified>,
    #[serde(skip)]
    check: Option<Check>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Verified {
    pub ok: bool,
    pub detail: String,
    /// What to do when it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

/// How to ask a service who we are.
#[derive(Debug, Clone)]
enum Check {
    Gh { host: String },
    Ssh { target: String },
    Aws,
}

pub fn whoami(ctx: &Ctx, verify: bool) -> Vec<Identity> {
    let mut out = github(ctx);
    out.extend(git_author(ctx));
    out.extend(ssh_remotes(ctx));
    out.extend(aws(ctx));
    if verify {
        for id in &mut out {
            if let Some(check) = id.check.clone() {
                let mut v = run_check(&check, ctx);
                if !v.ok {
                    v.fix = Some(fix_for(&check, id, ctx));
                }
                id.verified = Some(v);
            }
        }
    }
    out
}

// GitHub, as gh and the API see it.

fn gh_config_dir(ctx: &Ctx) -> PathBuf {
    if let Some(d) = ctx.var("GH_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    let base = ctx
        .var("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map_or_else(|| ctx.home.join(".config"), PathBuf::from);
    base.join("gh")
}

/// Hosts and their active user from gh's `hosts.yml`.
fn gh_hosts(text: &str) -> Vec<(String, Option<String>)> {
    let mut hosts: Vec<(String, Option<String>)> = Vec::new();
    for line in text.lines() {
        let indent = line.len() - line.trim_start().len();
        let t = line.trim();
        if indent == 0 && t.ends_with(':') && !t.starts_with('#') {
            hosts.push((t.trim_end_matches(':').trim_matches('"').to_owned(), None));
        } else if indent == 4
            && let Some(user) = t.strip_prefix("user:")
            && let Some(last) = hosts.last_mut()
        {
            let user = user.trim().trim_matches('"');
            if !user.is_empty() {
                last.1 = Some(user.to_owned());
            }
        }
    }
    hosts
}

/// The variable that overrides gh's stored login for `host`, if one is set.
fn gh_env_token(ctx: &Ctx, host: &str) -> Option<&'static str> {
    let names: &[&'static str] = if host == "github.com" || host.ends_with(".ghe.com") {
        &["GH_TOKEN", "GITHUB_TOKEN"]
    } else {
        &["GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN"]
    };
    names
        .iter()
        .copied()
        .find(|n| ctx.var(n).is_some_and(|v| !v.is_empty()))
}

fn github(ctx: &Ctx) -> Vec<Identity> {
    let dir = gh_config_dir(ctx);
    let file = dir.join("hosts.yml");
    let mut hosts = std::fs::read_to_string(&file)
        .map(|t| gh_hosts(&t))
        .unwrap_or_default();
    // A token in the environment counts even with no stored login.
    if !hosts.iter().any(|(h, _)| h == "github.com") && gh_env_token(ctx, "github.com").is_some() {
        hosts.push(("github.com".to_owned(), None));
    }
    hosts
        .into_iter()
        .map(|(host, user)| {
            let stored = user.as_deref().unwrap_or("no stored login");
            let (who, source, warning) = match gh_env_token(ctx, &host) {
                Some(var) => (
                    format!("token from {var}"),
                    "the environment".to_owned(),
                    Some(match &user {
                        Some(u) => format!(
                            "{var} is set, so gh and the GitHub API use it for {host} instead of {u}'s stored login. If they fail with bad credentials, unset {var}."
                        ),
                        None => format!("{var} is set, so gh uses it for {host}."),
                    }),
                ),
                None => (
                    stored.to_owned(),
                    format!("gh login in {}", tilde(&file, ctx)),
                    None,
                ),
            };
            Identity {
                service: "github",
                scope: host.clone(),
                who,
                source,
                warning,
                verified: None,
                check: Some(Check::Gh { host }),
            }
        })
        .collect()
}

// git's commit author.

/// `file:/Users/me/.gitconfig\tTrammel May` from `git config --show-origin`.
fn origin_value(out: &str) -> Option<(String, String)> {
    let (origin, value) = out.trim().split_once('\t')?;
    let origin = origin.strip_prefix("file:").unwrap_or(origin);
    Some((origin.to_owned(), value.to_owned()))
}

fn git_config(ctx: &Ctx, key: &str) -> Option<(String, String)> {
    git_out(&ctx.cwd, &["config", "--show-origin", "--get", key]).and_then(|o| origin_value(&o))
}

fn git_author(ctx: &Ctx) -> Option<Identity> {
    let name = git_config(ctx, "user.name");
    let email = git_config(ctx, "user.email");
    if name.is_none() && email.is_none() {
        return Some(Identity {
            service: "git",
            scope: "commits".into(),
            who: "nobody".into(),
            source: "no user.name or user.email".into(),
            warning: Some("git has no author set here: commits will fail or use a guessed address. Set user.name and user.email.".into()),
            verified: None,
            check: None,
        });
    }
    let who = format!(
        "{} <{}>",
        name.as_ref().map_or("(no name)", |n| n.1.as_str()),
        email.as_ref().map_or("no email", |e| e.1.as_str())
    );
    let mut origins: Vec<String> = [&name, &email]
        .into_iter()
        .flatten()
        .map(|(o, _)| tilde(Path::new(o), ctx))
        .collect();
    origins.dedup();
    let mut source = origins.join(", ");
    if git_out(&ctx.cwd, &["config", "--get", "commit.gpgsign"]).as_deref() == Some("true") {
        let format =
            git_out(&ctx.cwd, &["config", "--get", "gpg.format"]).unwrap_or_else(|| "gpg".into());
        source.push_str(&format!("; signs commits ({format})"));
    }
    Some(Identity {
        service: "git",
        scope: "commits".into(),
        who,
        source,
        warning: email
            .is_none()
            .then(|| "No user.email: commits will use a guessed address.".to_owned()),
        verified: None,
        check: None,
    })
}

// ssh keys for the repo's remotes.

/// `git@github.com:me/r.git` or `ssh://git@host:22/r` as `git@github.com`;
/// `None` for https and local remotes.
fn ssh_target(url: &str) -> Option<String> {
    if let Some(rest) = url.strip_prefix("ssh://") {
        let authority = rest.split('/').next()?;
        let authority = authority.rsplit_once(':').map_or(authority, |(h, _)| h);
        return Some(authority.to_owned());
    }
    if url.contains("://") {
        return None;
    }
    let (host, _) = url.split_once(':')?;
    (!host.contains('/')).then(|| host.to_owned())
}

/// Identity files and user from `ssh -G` output.
fn ssh_config(out: &str) -> (Vec<String>, Option<String>) {
    let mut files = Vec::new();
    let mut user = None;
    for line in out.lines() {
        if let Some(f) = line.strip_prefix("identityfile ") {
            files.push(f.trim().to_owned());
        } else if let Some(u) = line.strip_prefix("user ") {
            user = Some(u.trim().to_owned());
        }
    }
    (files, user)
}

fn ssh_remotes(ctx: &Ctx) -> Vec<Identity> {
    let Some(names) = git_out(&ctx.cwd, &["remote"]) else {
        return Vec::new();
    };
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for remote in names.lines() {
        let Some(url) = remote_url(&ctx.cwd, remote) else {
            continue;
        };
        let Some(target) = ssh_target(&url) else {
            continue;
        };
        if seen.contains(&target) {
            continue;
        }
        seen.push(target.clone());
        let host = target.rsplit_once('@').map_or(target.as_str(), |(_, h)| h);
        let mut cmd = Command::new("ssh");
        cmd.args(["-G", host]);
        let (files, _user) = run(cmd, Duration::from_secs(5))
            .map(|o| ssh_config(&o.stdout))
            .unwrap_or_default();
        let existing: Vec<String> = files
            .iter()
            .filter(|f| ctx.path(f).exists())
            .map(|f| tilde(&ctx.path(f), ctx))
            .collect();
        let who = if existing.is_empty() {
            "the ssh agent's keys".to_owned()
        } else {
            existing.join(", ")
        };
        out.push(Identity {
            service: "ssh",
            scope: format!("{} ({remote})", normalize_remote(&url)),
            who,
            source: format!("ssh -G {host}"),
            warning: None,
            verified: None,
            check: Some(Check::Ssh { target }),
        });
    }
    out
}

// AWS credentials.

fn aws(ctx: &Ctx) -> Option<Identity> {
    let profile = ctx
        .var("AWS_PROFILE")
        .or_else(|| ctx.var("AWS_DEFAULT_PROFILE"))
        .filter(|p| !p.is_empty());
    let keys = ctx.var("AWS_ACCESS_KEY_ID").is_some_and(|k| !k.is_empty());
    if keys {
        return Some(Identity {
            service: "aws",
            scope: "credentials".into(),
            who: "static keys".into(),
            source: "AWS_ACCESS_KEY_ID".into(),
            warning: profile.map(|p| {
                format!(
                    "AWS_ACCESS_KEY_ID is set, so the AWS CLI uses those keys, not profile {p}."
                )
            }),
            verified: None,
            check: Some(Check::Aws),
        });
    }
    if profile.is_none() && !ctx.home.join(".aws").exists() {
        return None;
    }
    Some(Identity {
        service: "aws",
        scope: "credentials".into(),
        who: format!("profile {}", profile.unwrap_or("default")),
        source: if profile.is_some() {
            "AWS_PROFILE".into()
        } else {
            "no AWS_PROFILE set".into()
        },
        warning: None,
        verified: None,
        check: Some(Check::Aws),
    })
}

// --verify

fn fix_for(check: &Check, id: &Identity, ctx: &Ctx) -> String {
    match check {
        Check::Gh { host } => match gh_env_token(ctx, host) {
            Some(var) => format!(
                "Unset {var} (or replace it with a valid token); gh then uses its stored login."
            ),
            None => format!("Log in again: gh auth login -h {host}"),
        },
        Check::Ssh { target } => format!(
            "Add a key the host accepts to the ssh agent (ssh-add), or set IdentityFile for {target} in ~/.ssh/config."
        ),
        Check::Aws if id.who == "static keys" => {
            "Replace or unset AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY.".into()
        }
        Check::Aws => {
            let profile = id.who.trim_start_matches("profile ");
            format!(
                "Refresh the credentials: aws sso login --profile {profile}, or update ~/.aws/credentials."
            )
        }
    }
}

fn run_check(check: &Check, ctx: &Ctx) -> Verified {
    let timeout = Duration::from_secs(10);
    let (mut cmd, label) = match check {
        Check::Gh { host } => {
            let mut c = Command::new("gh");
            c.args(["api", "user", "--hostname", host, "--jq", ".login"]);
            (c, "gh")
        }
        Check::Ssh { target } => {
            let mut c = Command::new("ssh");
            c.args([
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=5",
                target,
            ]);
            (c, "ssh")
        }
        Check::Aws => {
            let mut c = Command::new("aws");
            c.args([
                "sts",
                "get-caller-identity",
                "--query",
                "Arn",
                "--output",
                "text",
            ]);
            (c, "aws")
        }
    };
    cmd.envs(&ctx.vars).current_dir(&ctx.cwd);
    let out = match run(cmd, timeout) {
        Ok(o) => o,
        Err(e) => {
            return Verified {
                ok: false,
                detail: e,
                fix: None,
            };
        }
    };
    let first = |s: &str| s.trim().lines().next().unwrap_or("").to_owned();
    match check {
        // GitHub and GitLab greet by name, then exit 1 (no shell).
        Check::Ssh { .. } => match ssh_greeting(&out.stderr) {
            Some(user) => Verified {
                ok: true,
                detail: format!("authenticates as {user}"),
                fix: None,
            },
            None => Verified {
                ok: false,
                detail: non_empty(first(&out.stderr), || {
                    format!("{label} exited {:?}", out.code)
                }),
                fix: None,
            },
        },
        _ if out.code == Some(0) => Verified {
            ok: true,
            detail: format!("authenticates as {}", first(&out.stdout)),
            fix: None,
        },
        _ => Verified {
            ok: false,
            detail: non_empty(first(&out.stderr), || {
                format!("{label} exited {:?}", out.code)
            }),
            fix: None,
        },
    }
}

fn non_empty(s: String, or: impl FnOnce() -> String) -> String {
    if s.is_empty() { or() } else { s }
}

/// The user in `Hi me! You've successfully authenticated` (GitHub),
/// `Welcome to GitLab, @me!`, or `authenticated via ... logged in as me`.
fn ssh_greeting(stderr: &str) -> Option<String> {
    let word = |s: &str| -> String {
        s.chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
            .collect()
    };
    for line in stderr.lines() {
        if let Some(rest) = line.strip_prefix("Hi ") {
            return Some(word(rest)).filter(|w| !w.is_empty());
        }
        if let Some(i) = line.find("Welcome to GitLab, @") {
            return Some(word(&line[i + 20..])).filter(|w| !w.is_empty());
        }
        if let Some(i) = line.find("logged in as ") {
            return Some(word(&line[i + 13..])).filter(|w| !w.is_empty());
        }
    }
    None
}

/// `/Users/me/.gitconfig` as `~/.gitconfig`.
fn tilde(path: &Path, ctx: &Ctx) -> String {
    match path.strip_prefix(&ctx.home) {
        Ok(rest) if !ctx.home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_gh_hosts() {
        let text = "github.example.com:\n    git_protocol: ssh\n    users:\n        trammel-may:\n    user: trammel-may\ngithub.com:\n    users:\n        a:\n        b:\n    user: b\n";
        assert_eq!(
            gh_hosts(text),
            [
                (
                    "github.example.com".to_owned(),
                    Some("trammel-may".to_owned())
                ),
                ("github.com".to_owned(), Some("b".to_owned()))
            ]
        );
    }

    #[test]
    fn env_tokens_override_by_host() {
        let mut ctx = Ctx::default();
        ctx.vars.insert("GH_TOKEN".into(), "x".into());
        assert_eq!(gh_env_token(&ctx, "github.com"), Some("GH_TOKEN"));
        assert_eq!(gh_env_token(&ctx, "github.example.com"), None);
        ctx.vars.insert("GH_ENTERPRISE_TOKEN".into(), "y".into());
        assert_eq!(
            gh_env_token(&ctx, "github.example.com"),
            Some("GH_ENTERPRISE_TOKEN")
        );
    }

    #[test]
    fn reads_git_origins_and_ssh_config() {
        assert_eq!(
            origin_value("file:/Users/me/.gitconfig\tTrammel May\n"),
            Some(("/Users/me/.gitconfig".into(), "Trammel May".into()))
        );
        assert_eq!(
            ssh_target("git@github.com:me/r.git").as_deref(),
            Some("git@github.com")
        );
        assert_eq!(
            ssh_target("ssh://git@gitlab.com:2222/me/r").as_deref(),
            Some("git@gitlab.com")
        );
        assert_eq!(ssh_target("https://github.com/me/r"), None);
        assert_eq!(ssh_target("/srv/repos/r.git"), None);
        let (files, user) = ssh_config(
            "user git\nidentityfile ~/.ssh/id_ed25519\nidentityfile ~/.ssh/id_rsa\nport 22\n",
        );
        assert_eq!(files, ["~/.ssh/id_ed25519", "~/.ssh/id_rsa"]);
        assert_eq!(user.as_deref(), Some("git"));
    }

    #[test]
    fn reads_ssh_greetings() {
        assert_eq!(
            ssh_greeting("Hi trammel-may! You've successfully authenticated, but GitHub does not provide shell access.").as_deref(),
            Some("trammel-may")
        );
        assert_eq!(
            ssh_greeting("Welcome to GitLab, @tmay!").as_deref(),
            Some("tmay")
        );
        assert_eq!(ssh_greeting("Permission denied (publickey)."), None);
    }

    #[test]
    fn aws_keys_override_a_profile() {
        let mut ctx = Ctx::default();
        ctx.vars.insert("AWS_PROFILE".into(), "personal".into());
        ctx.vars.insert("AWS_ACCESS_KEY_ID".into(), "AKIA".into());
        let id = aws(&ctx).unwrap();
        assert_eq!(id.who, "static keys");
        assert!(id.warning.unwrap().contains("not profile personal"));
    }
}
