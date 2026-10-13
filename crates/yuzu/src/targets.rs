//! What each simple command would reach, and whether it would change it:
//! the kube context for `kubectl` and `helm`, the host for `ssh`, `scp`,
//! `sftp`, and `rsync`, the remote for `git push`, the profile for `aws`,
//! the directory for terraform and friends, and the tool for cloud CLIs
//! (docs/decisions/0013-yuzu-check.md, 0020-yuzu-infra-and-cloud-clis.md).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::config::Kind;
use crate::env::Ctx;
use crate::shell;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    Read,
    Write,
}

/// One thing a command would act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Target {
    pub kind: Kind,
    /// The context, host, remote URL, or profile name.
    pub name: String,
    pub access: Access,
    /// The simple command it came from, words joined by spaces.
    pub command: String,
    /// How the name was found, when it wasn't on the command line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Every target in the command line `src`, run from `ctx`.
pub fn find(src: &str, ctx: &Ctx) -> Vec<Target> {
    let mut ctx = ctx.clone();
    let mut out = Vec::new();
    for words in shell::commands(src) {
        visit(&words, &mut ctx, &mut out, 0);
    }
    out
}

/// The targets of one command given as words, already split.
pub fn find_words(words: &[String], ctx: &Ctx) -> Vec<Target> {
    let mut ctx = ctx.clone();
    let mut out = Vec::new();
    visit(words, &mut ctx, &mut out, 0);
    out
}

/// Programs that run the rest of their arguments as a command.
const WRAPPERS: &[&str] = &[
    "sudo",
    "doas",
    "env",
    "time",
    "nohup",
    "nice",
    "ionice",
    "command",
    "builtin",
    "exec",
    "timeout",
    "watch",
    "xargs",
    "caffeinate",
    "{",
    "}",
    "!",
    "then",
    "do",
    "else",
    "elif",
    "if",
    "while",
    "until",
    "noglob",
];

/// Wrapper options that take a value, so the value isn't taken as the command.
fn wrapper_takes_value(wrapper: &str, flag: &str) -> bool {
    match wrapper {
        "sudo" | "doas" => matches!(
            flag,
            "-u" | "-g" | "-h" | "-p" | "-C" | "-D" | "-U" | "-r" | "-t"
        ),
        "env" => matches!(flag, "-u" | "-C" | "-S"),
        "nice" | "ionice" => matches!(flag, "-n" | "-c"),
        "timeout" => matches!(flag, "-s" | "-k" | "--signal" | "--kill-after"),
        "watch" => matches!(flag, "-n" | "-d" | "--interval"),
        "xargs" => matches!(flag, "-I" | "-L" | "-n" | "-P" | "-d" | "-E" | "-s" | "-a"),
        _ => false,
    }
}

fn visit(words: &[String], ctx: &mut Ctx, out: &mut Vec<Target>, depth: usize) {
    if depth > 8 {
        return;
    }
    // Leading NAME=value words set the environment for this command only.
    let mut local = ctx.clone();
    let mut rest = words;
    while let Some((first, tail)) = rest.split_first() {
        if let Some((name, value)) = assignment(first) {
            local.vars.insert(name.to_owned(), value.to_owned());
            rest = tail;
        } else {
            break;
        }
    }
    let Some((program, args)) = rest.split_first() else {
        // Only assignments: they last for the rest of the line.
        *ctx = local;
        return;
    };
    let name = program.rsplit('/').next().unwrap_or(program);
    let command = rest.join(" ");
    match name {
        _ if WRAPPERS.contains(&name) => {
            let inner = skip_wrapper(name, args, &mut local);
            visit(inner, &mut local, out, depth + 1);
        }
        "export" => {
            for a in args {
                if let Some((n, v)) = assignment(a) {
                    ctx.vars.insert(n.to_owned(), v.to_owned());
                }
            }
        }
        "unset" => {
            for a in args {
                ctx.vars.remove(a);
            }
        }
        "cd" | "pushd" => {
            let dir = args.iter().find(|a| !a.starts_with('-'));
            ctx.cwd = dir.map_or_else(|| ctx.home.clone(), |d| ctx.path(d));
        }
        "bash" | "sh" | "zsh" | "dash" | "ksh" => {
            if let Some(script) = shell_c_script(args) {
                for words in shell::commands(script) {
                    visit(&words, &mut local, out, depth + 1);
                }
            }
        }
        "eval" => {
            for words in shell::commands(&args.join(" ")) {
                visit(&words, &mut local, out, depth + 1);
            }
        }
        "kubectl" | "oc" => out.extend(kubectl(args, &local, command)),
        "helm" => out.extend(helm(args, &local, command)),
        "ssh" | "mosh" | "sftp" => out.extend(ssh(args, command)),
        "scp" | "rsync" => out.extend(copy(args, command)),
        "git" => out.extend(git(args, &local, command)),
        "aws" => out.extend(aws(args, &local, command)),
        "terraform" | "tofu" | "terragrunt" | "pulumi" => {
            out.extend(infra(args, &local, command));
        }
        "gcloud" | "az" | "doctl" | "flyctl" | "fly" | "heroku" | "vercel" | "netlify"
        | "wrangler" => out.extend(cloud_cli(&words[0], args, command)),
        _ => {}
    }
}

fn assignment(word: &str) -> Option<(&str, &str)> {
    let (name, value) = word.split_once('=')?;
    let valid = !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    valid.then_some((name, value))
}

/// The command a wrapper runs, after its own options and arguments.
fn skip_wrapper<'a>(wrapper: &str, args: &'a [String], local: &mut Ctx) -> &'a [String] {
    let mut i = 0;
    // `timeout 30 cmd`: the duration comes first.
    let mut needs_positional = usize::from(wrapper == "timeout");
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            return &args[i + 1..];
        }
        if wrapper == "env"
            && let Some((n, v)) = assignment(a)
        {
            local.vars.insert(n.to_owned(), v.to_owned());
            i += 1;
            continue;
        }
        if a.starts_with('-') && a.len() > 1 && !matches!(wrapper, "{" | "}" | "!") {
            i += if wrapper_takes_value(wrapper, a) {
                2
            } else {
                1
            };
            continue;
        }
        if needs_positional > 0 {
            needs_positional -= 1;
            i += 1;
            continue;
        }
        break;
    }
    &args[i.min(args.len())..]
}

/// The script in `sh -c 'script'` (also `-lc`, `-ec`, and so on).
fn shell_c_script(args: &[String]) -> Option<&str> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if a.starts_with('-') && !a.starts_with("--") && a.contains('c') {
            return iter.next().map(String::as_str);
        }
        if !a.starts_with('-') {
            return None;
        }
    }
    None
}

/// The value of `--name value` or `--name=value`, the last one if repeated.
fn flag_value<'a>(args: &'a [String], names: &[&str]) -> Option<&'a str> {
    let mut found = None;
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if a == "--" {
            break;
        }
        for n in names {
            if a == n {
                found = iter.clone().next().map(String::as_str);
            } else if let Some(v) = a.strip_prefix(n).and_then(|r| r.strip_prefix('=')) {
                found = Some(v);
            }
        }
    }
    found
}

/// Positional arguments, skipping flags and the values of `value_flags`.
fn positionals<'a>(args: &'a [String], value_flags: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if a == "--" {
            out.extend(iter.map(String::as_str));
            break;
        }
        if a.starts_with('-') && a.len() > 1 {
            if !a.contains('=') && value_flags.contains(&a.as_str()) {
                iter.next();
            }
            continue;
        }
        out.push(a.as_str());
    }
    out
}

// kubectl and helm

const KUBECTL_VALUE_FLAGS: &[&str] = &[
    "--context",
    "--kubeconfig",
    "-n",
    "--namespace",
    "-s",
    "--server",
    "--cluster",
    "--user",
    "--token",
    "--as",
    "--as-group",
    "--as-uid",
    "--request-timeout",
    "--certificate-authority",
    "--client-certificate",
    "--client-key",
    "--cache-dir",
    "-v",
    "--v",
    "--tls-server-name",
    "--username",
    "--password",
    "--profile",
    "--profile-output",
    "--log-file",
    "--vmodule",
];

/// kubectl subcommands that only read from the cluster.
const KUBECTL_READS: &[&str] = &[
    "get",
    "describe",
    "logs",
    "top",
    "explain",
    "api-resources",
    "api-versions",
    "cluster-info",
    "version",
    "diff",
    "events",
    "wait",
];

/// kubectl subcommands that never contact a cluster.
const KUBECTL_LOCAL: &[&str] = &[
    "config",
    "completion",
    "kustomize",
    "plugin",
    "help",
    "options",
    "alpha",
];

fn kubectl(args: &[String], ctx: &Ctx, command: String) -> Option<Target> {
    let pos = positionals(args, KUBECTL_VALUE_FLAGS);
    let verb = pos.first().copied().unwrap_or("");
    if verb.is_empty() || KUBECTL_LOCAL.contains(&verb) {
        return None;
    }
    let second = pos.get(1).copied().unwrap_or("");
    let read = KUBECTL_READS.contains(&verb)
        || (verb == "rollout" && matches!(second, "status" | "history"))
        || (verb == "auth" && matches!(second, "can-i" | "whoami"))
        || dry_run(args);
    let access = if read { Access::Read } else { Access::Write };
    let (name, source) = kube_context(args, ctx, &["--context"])?;
    Some(Target {
        kind: Kind::KubeContext,
        name,
        access,
        command,
        source,
    })
}

/// `--dry-run`, `--dry-run=client`, or `--dry-run=server`: nothing changes.
fn dry_run(args: &[String]) -> bool {
    args.iter()
        .any(|a| a == "--dry-run" || a.strip_prefix("--dry-run=").is_some_and(|v| v != "none"))
}

const HELM_VALUE_FLAGS: &[&str] = &[
    "--kube-context",
    "--kubeconfig",
    "-n",
    "--namespace",
    "--registry-config",
    "--repository-config",
    "--repository-cache",
    "--burst-limit",
    "--kube-apiserver",
    "--kube-as-user",
    "--kube-as-group",
    "--kube-token",
    "--kube-ca-file",
    "--kube-tls-server-name",
    "--qps",
];

const HELM_READS: &[&str] = &[
    "list", "ls", "status", "get", "history", "hist", "show", "inspect", "template", "lint",
    "version", "verify",
];

/// helm subcommands that never contact a cluster.
const HELM_LOCAL: &[&str] = &[
    "repo",
    "search",
    "plugin",
    "package",
    "dependency",
    "dep",
    "pull",
    "fetch",
    "create",
    "env",
    "completion",
    "help",
    "registry",
    "push",
];

fn helm(args: &[String], ctx: &Ctx, command: String) -> Option<Target> {
    let pos = positionals(args, HELM_VALUE_FLAGS);
    let verb = pos.first().copied().unwrap_or("");
    if verb.is_empty() || HELM_LOCAL.contains(&verb) {
        return None;
    }
    let read = HELM_READS.contains(&verb) || dry_run(args);
    let access = if read { Access::Read } else { Access::Write };
    let (name, source) = kube_context(args, ctx, &["--kube-context"])?;
    Some(Target {
        kind: Kind::KubeContext,
        name,
        access,
        command,
        source,
    })
}

/// The context a kubectl-like command would use: the flag, else the
/// kubeconfig's `current-context`. `None` when there's no context at all
/// (the command would fail without reaching anything).
fn kube_context(args: &[String], ctx: &Ctx, flags: &[&str]) -> Option<(String, Option<String>)> {
    if let Some(name) = flag_value(args, flags) {
        return Some((name.to_owned(), None));
    }
    let files: Vec<PathBuf> = if let Some(f) = flag_value(args, &["--kubeconfig"]) {
        vec![ctx.path(f)]
    } else {
        kubeconfig_files(ctx)
    };
    match current_context(&files) {
        Ok(Some((name, file))) => {
            Some((name, Some(format!("current-context in {}", file.display()))))
        }
        Ok(None) => None,
        // Can't tell where it would go, so say so; unknown counts as risky.
        Err(e) => Some(("(unknown)".to_owned(), Some(e))),
    }
}

/// `$KUBECONFIG` (colon-separated), else `~/.kube/config`.
pub fn kubeconfig_files(ctx: &Ctx) -> Vec<PathBuf> {
    match ctx.var("KUBECONFIG").filter(|v| !v.is_empty()) {
        Some(list) => list
            .split(':')
            .filter(|p| !p.is_empty())
            .map(|p| ctx.path(p))
            .collect(),
        None => vec![ctx.home.join(".kube").join("config")],
    }
}

/// The first `current-context` set across `files`, the way kubectl merges
/// them. Missing files are skipped, as kubectl does.
pub fn current_context(files: &[PathBuf]) -> Result<Option<(String, PathBuf)>, String> {
    for file in files {
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("couldn't read {}: {e}", file.display())),
        };
        if let Some(name) = top_level_value(&text, "current-context").filter(|n| !n.is_empty()) {
            return Ok(Some((name, file.clone())));
        }
    }
    Ok(None)
}

/// A top-level `key: value` from YAML, unquoted. Enough for kubeconfig,
/// which tools write in plain block style.
fn top_level_value(yaml: &str, key: &str) -> Option<String> {
    yaml.lines().find_map(|line| {
        let rest = line.strip_prefix(key)?.strip_prefix(':')?;
        let value = rest.split(" #").next().unwrap_or(rest).trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        Some(value.to_owned())
    })
}

// ssh, sftp, scp, rsync

const SSH_VALUE_FLAGS: &[&str] = &[
    "-b", "-c", "-D", "-E", "-e", "-F", "-I", "-i", "-J", "-L", "-l", "-m", "-O", "-o", "-p", "-Q",
    "-R", "-S", "-W", "-w", "-B", "-P",
];

/// `user@host`, `ssh://user@host:22`, or `host` to `host`, lowercased.
fn host_of(dest: &str) -> String {
    let dest = dest.strip_prefix("ssh://").unwrap_or(dest);
    let dest = dest.rsplit_once('@').map_or(dest, |(_, h)| h);
    let host = if let Some(bracketed) = dest.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or(bracketed)
    } else {
        dest.split([':', '/']).next().unwrap_or(dest)
    };
    host.to_ascii_lowercase()
}

/// Any command over ssh can change the host, so it counts as a write.
fn ssh(args: &[String], command: String) -> Option<Target> {
    let dest = positionals(args, SSH_VALUE_FLAGS).first().copied()?;
    Some(Target {
        kind: Kind::SshHost,
        name: host_of(dest),
        access: Access::Write,
        command,
        source: None,
    })
}

/// `host:path` (or `user@host:path`, `host::module`): a remote path in scp
/// or rsync. Local paths with a colon start with `/` or `.`.
fn remote_host(arg: &str) -> Option<String> {
    if arg.starts_with(['/', '.', '~']) || arg.starts_with("rsync://") {
        return arg
            .strip_prefix("rsync://")
            .map(|r| host_of(r.split('/').next().unwrap_or(r)));
    }
    let (head, _) = arg.split_once(':')?;
    (!head.is_empty() && !head.contains('/')).then(|| host_of(head))
}

/// The last argument is the destination: a remote one is a write, a remote
/// source only a read.
fn copy(args: &[String], command: String) -> Vec<Target> {
    let value_flags: &[&str] = &[
        "-e",
        "--rsh",
        "-i",
        "-o",
        "-P",
        "-p",
        "-F",
        "-c",
        "-l",
        "-S",
        "-J",
        "--exclude",
        "--include",
        "--filter",
        "-f",
        "--port",
        "--password-file",
        "--log-file",
    ];
    let pos = positionals(args, value_flags);
    let last = pos.len().saturating_sub(1);
    let mut out: Vec<Target> = Vec::new();
    for (i, a) in pos.iter().enumerate() {
        let Some(host) = remote_host(a) else { continue };
        let access = if i == last {
            Access::Write
        } else {
            Access::Read
        };
        if let Some(t) = out.iter_mut().find(|t| t.name == host) {
            if access == Access::Write {
                t.access = Access::Write;
            }
            continue;
        }
        out.push(Target {
            kind: Kind::SshHost,
            name: host,
            access,
            command: command.clone(),
            source: None,
        });
    }
    out
}

// git

const GIT_VALUE_FLAGS: &[&str] = &["-C", "-c", "--git-dir", "--work-tree", "--namespace"];

/// Only `git push` writes somewhere else; everything else is local or a read.
fn git(args: &[String], ctx: &Ctx, command: String) -> Option<Target> {
    let mut dir = ctx.cwd.clone();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "-C" {
            if let Some(d) = args.get(i + 1) {
                dir = dir.join(d);
            }
            i += 2;
        } else if GIT_VALUE_FLAGS.contains(&a) {
            i += 2;
        } else if a.starts_with('-') {
            i += 1;
        } else {
            break;
        }
    }
    if args.get(i).map(String::as_str) != Some("push") {
        return None;
    }
    let push_args = &args[i + 1..];
    let remote = flag_value(push_args, &["--repo"])
        .map(str::to_owned)
        .or_else(|| {
            positionals(
                push_args,
                &["-o", "--push-option", "--receive-pack", "--exec"],
            )
            .first()
            .map(|s| (*s).to_owned())
        });
    let (url, source) = match remote {
        Some(r) if looks_like_url(&r) => (r, None),
        Some(r) => remote_url(&dir, &r).map_or_else(
            || (r.clone(), Some(format!("no remote named {r}"))),
            |u| (u, Some(format!("remote {r}"))),
        ),
        None => {
            let r = default_push_remote(&dir);
            remote_url(&dir, &r).map_or_else(
                || (r.clone(), Some(format!("no remote named {r}"))),
                |u| (u, Some(format!("remote {r}, the default for this branch"))),
            )
        }
    };
    Some(Target {
        kind: Kind::GitRemote,
        name: normalize_remote(&url),
        access: Access::Write,
        command,
        source,
    })
}

fn looks_like_url(r: &str) -> bool {
    r.contains("://") || r.contains(':') || r.starts_with('/') || r.starts_with('.')
}

pub(crate) fn git_out(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !text.is_empty()).then_some(text)
}

pub(crate) fn remote_url(dir: &Path, remote: &str) -> Option<String> {
    git_out(dir, &["remote", "get-url", "--push", remote])
}

/// Where a bare `git push` goes: the branch's push remote, else
/// `remote.pushDefault`, else the branch's upstream remote, else `origin`,
/// else the only remote there is.
fn default_push_remote(dir: &Path) -> String {
    if let Some(branch) = git_out(dir, &["symbolic-ref", "--short", "-q", "HEAD"]) {
        for key in [
            format!("branch.{branch}.pushRemote"),
            "remote.pushDefault".to_owned(),
            format!("branch.{branch}.remote"),
        ] {
            if let Some(r) = git_out(dir, &["config", "--get", &key]) {
                return r;
            }
        }
    }
    let remotes = git_out(dir, &["remote"]).unwrap_or_default();
    let remotes: Vec<&str> = remotes.lines().collect();
    match remotes.as_slice() {
        [only] if *only != "origin" => (*only).to_owned(),
        _ => "origin".to_owned(),
    }
}

/// `git@github.com:me/repo.git` and `https://github.com/me/repo` both become
/// `github.com/me/repo`, so one pattern matches every spelling.
pub fn normalize_remote(url: &str) -> String {
    let mut s = url.trim();
    if let Some((_, rest)) = s.split_once("://") {
        s = rest;
    } else if let Some((host, path)) = s.split_once(':')
        && !host.contains('/')
    {
        // scp-like: host:path
        let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
        return finish_remote(&format!("{host}/{}", path.trim_start_matches('/')));
    }
    let (host, path) = s.split_once('/').unwrap_or((s, ""));
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next().unwrap_or(host);
    finish_remote(&format!("{host}/{path}"))
}

fn finish_remote(s: &str) -> String {
    let s = s.trim_end_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s);
    s.to_ascii_lowercase()
}

// aws

const AWS_VALUE_FLAGS: &[&str] = &[
    "--profile",
    "--region",
    "--output",
    "--endpoint-url",
    "--query",
    "--cli-read-timeout",
    "--cli-connect-timeout",
    "--color",
    "--ca-bundle",
    "--cli-binary-format",
];

/// Operations that only read, by prefix.
const AWS_READ_PREFIXES: &[&str] = &[
    "describe-",
    "list-",
    "get-",
    "head-",
    "lookup-",
    "search-",
    "batch-get-",
    "filter-",
];

fn aws(args: &[String], ctx: &Ctx, command: String) -> Option<Target> {
    let pos = positionals(args, AWS_VALUE_FLAGS);
    let service = pos.first().copied()?;
    let op = pos.get(1).copied().unwrap_or("");
    if matches!(service, "configure" | "help" | "sso") {
        return None;
    }
    let read = match service {
        "sts" => true,
        "s3" => match op {
            "ls" | "presign" => true,
            // A copy or sync to a local path only reads the bucket.
            "cp" | "sync" => !pos.last().is_some_and(|d| d.starts_with("s3://")),
            _ => false,
        },
        _ => {
            AWS_READ_PREFIXES.iter().any(|p| op.starts_with(p))
                || matches!(op, "scan" | "query" | "tail" | "help" | "wait")
        }
    };
    let (name, source) = match flag_value(args, &["--profile"]) {
        Some(p) => (p.to_owned(), None),
        None => match ctx
            .var("AWS_PROFILE")
            .or_else(|| ctx.var("AWS_DEFAULT_PROFILE"))
        {
            Some(p) if !p.is_empty() => (p.to_owned(), Some("AWS_PROFILE".to_owned())),
            _ => (
                "default".to_owned(),
                Some("no --profile or AWS_PROFILE".to_owned()),
            ),
        },
    };
    Some(Target {
        kind: Kind::AwsProfile,
        name,
        access: if read { Access::Read } else { Access::Write },
        command,
        source,
    })
}

/// Subcommands that only read or plan, for terraform, tofu, terragrunt, and
/// pulumi. `init` downloads providers into the directory, not the cloud.
const INFRA_READS: &[&str] = &[
    "plan",
    "show",
    "validate",
    "output",
    "fmt",
    "init",
    "version",
    "providers",
    "graph",
    "console",
    "get",
    "preview",
    "whoami",
    "about",
    "login",
    "logout",
    "-help",
    "--help",
    "help",
];

/// The directory an infrastructure-as-code tool works in, written with `~`
/// for the home directory so `~/homelab/*` can declare it.
fn infra(args: &[String], ctx: &Ctx, command: String) -> Option<Target> {
    let mut dir = ctx.cwd.clone();
    let mut source = Some("the working directory".to_owned());
    for (i, a) in args.iter().enumerate() {
        let given = a
            .strip_prefix("-chdir=")
            .or_else(|| a.strip_prefix("--cwd="))
            .or_else(|| {
                (a == "-C" || a == "--cwd")
                    .then(|| args.get(i + 1).map(String::as_str))
                    .flatten()
            });
        if let Some(d) = given {
            dir = ctx.path(d);
            source = None;
        }
    }
    let pos: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let sub = pos.first().copied().unwrap_or("");
    let read = sub.is_empty()
        || INFRA_READS.contains(&sub)
        // terraform state list/show, workspace list/show, pulumi stack ls.
        || (matches!(sub, "state" | "workspace" | "stack" | "config")
            && matches!(pos.get(1).copied(), None | Some("list" | "ls" | "show" | "select" | "get" | "output")));
    let dir = normalize(&dir);
    let shown = match dir.strip_prefix(&ctx.home) {
        Ok(rest) if !ctx.home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => dir.display().to_string(),
    };
    Some(Target {
        kind: Kind::InfraDir,
        name: shown.trim_end_matches('/').to_owned(),
        access: if read { Access::Read } else { Access::Write },
        command,
        source,
    })
}

/// `a/b/../c` as `a/c`, without touching the filesystem.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Words that mark a cloud CLI call as a read.
const CLI_READS: &[&str] = &[
    "list", "ls", "describe", "show", "get", "view", "status", "info", "logs", "whoami", "version",
    "help", "--help", "-h", "auth", "login", "config", "account", "inspect", "open",
];

/// A cloud CLI acts on the account its own config holds, which yuzu can't
/// read; the tool itself is the target.
fn cloud_cli(tool: &str, args: &[String], command: String) -> Option<Target> {
    let name = if tool == "fly" { "flyctl" } else { tool };
    let pos: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| !a.starts_with('-') || CLI_READS.contains(a))
        .take(4)
        .collect();
    let read = pos.is_empty() || pos.iter().any(|w| CLI_READS.contains(w));
    Some(Target {
        kind: Kind::CliTool,
        name: name.to_owned(),
        access: if read { Access::Read } else { Access::Write },
        command,
        source: Some(format!("the account in {name}'s own config")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(home: &Path) -> Ctx {
        Ctx {
            vars: Default::default(),
            cwd: home.to_path_buf(),
            home: home.to_path_buf(),
        }
    }

    fn kubeconfig(home: &Path, current: &str) {
        std::fs::create_dir_all(home.join(".kube")).unwrap();
        std::fs::write(
            home.join(".kube/config"),
            format!("apiVersion: v1\nclusters: []\ncurrent-context: {current}\nkind: Config\n"),
        )
        .unwrap();
    }

    fn summary(ts: &[Target]) -> Vec<(String, Access)> {
        ts.iter().map(|t| (t.name.clone(), t.access)).collect()
    }

    #[test]
    fn kubectl_reads_and_writes() {
        let home = tempfile::tempdir().unwrap();
        kubeconfig(home.path(), "work-nonprod");
        let c = ctx(home.path());
        let w = |s: &str| summary(&find(s, &c));
        assert_eq!(
            w("kubectl get pods -A"),
            [("work-nonprod".into(), Access::Read)]
        );
        assert_eq!(
            w("kubectl -n x apply -f y.yaml"),
            [("work-nonprod".into(), Access::Write)]
        );
        assert_eq!(
            w("kubectl --context k3s-pis delete pod x"),
            [("k3s-pis".into(), Access::Write)]
        );
        assert_eq!(
            w("kubectl --context=k3s-pis rollout status deploy/x"),
            [("k3s-pis".into(), Access::Read)]
        );
        assert_eq!(
            w("kubectl rollout restart deploy/x"),
            [("work-nonprod".into(), Access::Write)]
        );
        assert_eq!(
            w("kubectl apply -f x --dry-run=server"),
            [("work-nonprod".into(), Access::Read)]
        );
        assert_eq!(
            w("kubectl exec -it pod -- sh"),
            [("work-nonprod".into(), Access::Write)]
        );
        assert_eq!(
            w("kubectl frobnicate"),
            [("work-nonprod".into(), Access::Write)]
        );
        assert!(w("kubectl config use-context k3s-pis").is_empty());
        assert!(w("kubectl").is_empty());
    }

    #[test]
    fn kubeconfig_from_the_environment_and_flags() {
        let home = tempfile::tempdir().unwrap();
        kubeconfig(home.path(), "default-ctx");
        let other = home.path().join("pis.yaml");
        std::fs::write(&other, "current-context: \"k3s-pis\"\n").unwrap();
        let c = ctx(home.path());
        let name = |s: &str| find(s, &c)[0].name.clone();
        assert_eq!(name("KUBECONFIG=pis.yaml kubectl apply -f x"), "k3s-pis");
        assert_eq!(
            name("kubectl --kubeconfig ~/pis.yaml apply -f x"),
            "k3s-pis"
        );
        assert_eq!(
            name("export KUBECONFIG=/nope:pis.yaml; kubectl get pods"),
            "k3s-pis"
        );
        // The assignment lasts only for its own command.
        assert_eq!(
            summary(&find("KUBECONFIG=pis.yaml true; kubectl get pods", &c))[0].0,
            "default-ctx"
        );
    }

    #[test]
    fn no_kubeconfig_means_no_target() {
        let home = tempfile::tempdir().unwrap();
        assert!(find("kubectl apply -f x", &ctx(home.path())).is_empty());
    }

    #[test]
    fn looks_through_wrappers_and_shells() {
        let home = tempfile::tempdir().unwrap();
        kubeconfig(home.path(), "work");
        let c = ctx(home.path());
        let n = |s: &str| find(s, &c).len();
        assert_eq!(n("sudo -u root kubectl delete ns x"), 1);
        assert_eq!(n("timeout 30 kubectl delete ns x"), 1);
        // env's assignment points kubectl at a file that doesn't exist.
        assert_eq!(n("env KUBECONFIG=missing FOO=1 kubectl get pods"), 0);
        assert_eq!(n("bash -lc 'cd /tmp && kubectl apply -f x'"), 1);
        assert_eq!(n("watch -n 2 kubectl get pods"), 1);
        assert_eq!(n("for i in 1 2; do kubectl delete pod $i; done"), 1);
        assert_eq!(n("echo kubectl delete everything"), 0);
        assert_eq!(n("/usr/local/bin/kubectl get pods"), 1);
    }

    #[test]
    fn helm() {
        let home = tempfile::tempdir().unwrap();
        kubeconfig(home.path(), "work");
        let c = ctx(home.path());
        let w = |s: &str| summary(&find(s, &c));
        assert_eq!(w("helm list -A"), [("work".into(), Access::Read)]);
        assert_eq!(
            w("helm upgrade --install x ./chart --kube-context k3s-pis"),
            [("k3s-pis".into(), Access::Write)]
        );
        assert!(w("helm repo add x https://x").is_empty());
        assert!(w("helm template x ./chart").len() == 1);
    }

    #[test]
    fn ssh_hosts() {
        let c = ctx(Path::new("/"));
        let w = |s: &str| summary(&find(s, &c));
        assert_eq!(
            w("ssh samwise 'sudo k3s kubectl get pods'"),
            [("samwise".into(), Access::Write)]
        );
        assert_eq!(
            w("ssh -i key -p 2222 pi@10.0.0.8 uptime"),
            [("10.0.0.8".into(), Access::Write)]
        );
        assert_eq!(
            w("ssh ssh://admin@Bastion:22"),
            [("bastion".into(), Access::Write)]
        );
        assert_eq!(
            w("scp samwise:/etc/x ./x"),
            [("samwise".into(), Access::Read)]
        );
        assert_eq!(w("scp ./x merry:/tmp/"), [("merry".into(), Access::Write)]);
        assert_eq!(
            w("rsync -av -e ssh ./dir pippin:dir/"),
            [("pippin".into(), Access::Write)]
        );
        assert!(w("scp ./a ./b").is_empty());
        assert_eq!(w("sftp samwise"), [("samwise".into(), Access::Write)]);
    }

    #[test]
    fn git_push_resolves_the_remote() {
        let repo = tempfile::tempdir().unwrap();
        let g = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(repo.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        g(&["init", "-q", "-b", "main"]);
        g(&["remote", "add", "upstream", "git@github.com:Me/Repo.git"]);
        g(&[
            "remote",
            "add",
            "origin",
            "https://gitlab.example.com/work/app.git",
        ]);
        let c = ctx(repo.path());
        let name = |s: &str| find(s, &c).first().map(|t| t.name.clone());
        assert_eq!(
            name("git push upstream main").as_deref(),
            Some("github.com/me/repo")
        );
        assert_eq!(
            name("git push").as_deref(),
            Some("gitlab.example.com/work/app")
        );
        assert_eq!(
            name("git -c x=y push -u upstream HEAD").as_deref(),
            Some("github.com/me/repo")
        );
        g(&["remote", "remove", "origin"]);
        assert_eq!(name("git push").as_deref(), Some("github.com/me/repo"));
        g(&[
            "remote",
            "add",
            "origin",
            "https://gitlab.example.com/work/app.git",
        ]);
        g(&["config", "remote.pushDefault", "upstream"]);
        assert_eq!(
            name("git push --force").as_deref(),
            Some("github.com/me/repo")
        );
        assert_eq!(name("git push nope").as_deref(), Some("nope"));
        assert_eq!(name("git status"), None);
        assert_eq!(name("git fetch origin"), None);
    }

    #[test]
    fn remotes_normalize() {
        for url in [
            "git@github.com:trammel-may/mochi-donut.git",
            "https://github.com/trammel-may/mochi-donut",
            "ssh://git@github.com:22/trammel-may/mochi-donut.git",
            "https://user@GitHub.com/trammel-may/mochi-donut/",
        ] {
            assert_eq!(
                normalize_remote(url),
                "github.com/trammel-may/mochi-donut",
                "{url}"
            );
        }
    }

    #[test]
    fn infra_tools_act_on_their_directory() {
        let mut c = ctx(Path::new("/home/me/homelab/infra"));
        c.home = PathBuf::from("/home/me");
        let w = |s: &str, c: &Ctx| summary(&find(s, c));
        assert_eq!(
            w("terraform plan", &c),
            [("~/homelab/infra".into(), Access::Read)]
        );
        assert_eq!(
            w("terraform apply -auto-approve", &c),
            [("~/homelab/infra".into(), Access::Write)]
        );
        assert_eq!(
            w("terraform -chdir=/work/prod destroy", &c),
            [("/work/prod".into(), Access::Write)]
        );
        assert_eq!(
            w("terraform state list", &c),
            [("~/homelab/infra".into(), Access::Read)]
        );
        assert_eq!(
            w("terraform state rm aws_instance.a", &c),
            [("~/homelab/infra".into(), Access::Write)]
        );
        assert_eq!(
            w("pulumi up --cwd ../app", &c),
            [("~/homelab/app".into(), Access::Write)]
        );
        assert_eq!(
            w("cd /work/prod && tofu apply", &c),
            [("/work/prod".into(), Access::Write)]
        );
    }

    #[test]
    fn cloud_clis_are_their_own_target() {
        let c = ctx(Path::new("/"));
        let w = |s: &str| summary(&find(s, &c));
        assert_eq!(
            w("gcloud compute instances list"),
            [("gcloud".into(), Access::Read)]
        );
        assert_eq!(
            w("gcloud compute instances delete vm-1"),
            [("gcloud".into(), Access::Write)]
        );
        assert_eq!(w("fly deploy"), [("flyctl".into(), Access::Write)]);
        assert_eq!(w("az account show"), [("az".into(), Access::Read)]);
        assert_eq!(w("heroku --help"), [("heroku".into(), Access::Read)]);
    }

    #[test]
    fn aws_profiles() {
        let mut c = ctx(Path::new("/"));
        let w = |s: &str, c: &Ctx| summary(&find(s, c));
        assert_eq!(w("aws s3 ls", &c), [("default".into(), Access::Read)]);
        assert_eq!(
            w("aws --profile prod ec2 describe-instances", &c),
            [("prod".into(), Access::Read)]
        );
        assert_eq!(
            w("aws ec2 terminate-instances --instance-ids i-1", &c),
            [("default".into(), Access::Write)]
        );
        assert_eq!(
            w("aws s3 cp ./x s3://bucket/x", &c),
            [("default".into(), Access::Write)]
        );
        assert_eq!(
            w("aws s3 cp s3://bucket/x ./x", &c),
            [("default".into(), Access::Read)]
        );
        c.vars.insert("AWS_PROFILE".into(), "sandbox".into());
        assert_eq!(
            w("aws sts get-caller-identity", &c),
            [("sandbox".into(), Access::Read)]
        );
        assert!(w("aws configure list", &c).is_empty());
    }
}
