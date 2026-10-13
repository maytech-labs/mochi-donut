//! `yuzu wait`: block until a condition holds, or a timeout passes. Agents
//! wait on CI, deploys, ports, and pods all the time, and hand-written poll
//! loops are easy to get wrong (docs/decisions/0023-yuzu-wait.md).

use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::env::Ctx;
use crate::proc::run;
use crate::targets::{git_out, normalize_remote, remote_url};

/// What to wait for.
#[derive(Debug, Clone)]
pub enum Condition {
    /// A command exits 0.
    Cmd(Vec<String>),
    /// A TCP connection to `host:port` succeeds.
    Port(String),
    /// A URL answers with `status` (or any 2xx/3xx).
    Url { url: String, status: Option<u16> },
    /// A path exists.
    File(PathBuf),
    /// CI for a PR number, branch, tag, or commit finishes.
    Ci {
        what: Option<String>,
        repo: Option<String>,
    },
}

impl Condition {
    pub fn describe(&self) -> String {
        match self {
            Condition::Cmd(c) => format!("`{}` to succeed", c.join(" ")),
            Condition::Port(p) => format!("port {p} to accept connections"),
            Condition::Url { url, status } => match status {
                Some(s) => format!("{url} to answer {s}"),
                None => format!("{url} to answer 2xx or 3xx"),
            },
            Condition::File(p) => format!("{} to exist", p.display()),
            Condition::Ci { what, .. } => {
                format!("CI for {} to finish", what.as_deref().unwrap_or("HEAD"))
            }
        }
    }

    /// How often to look, unless told otherwise.
    pub fn default_every(&self) -> Duration {
        Duration::from_secs(match self {
            Condition::Cmd(_) | Condition::Port(_) | Condition::File(_) => 5,
            Condition::Url { .. } => 10,
            Condition::Ci { .. } => 30,
        })
    }
}

/// One look at the condition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Observation {
    /// The condition holds.
    pub met: bool,
    /// It can never hold now (CI failed), so stop waiting.
    pub settled: bool,
    /// What was seen, for people: `1 of 2 checks pending (check (macos-15))`.
    pub detail: String,
    /// Failed CI checks or jobs, with their links.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<String>,
}

impl Observation {
    fn pending(detail: impl Into<String>) -> Observation {
        Observation {
            met: false,
            settled: false,
            detail: detail.into(),
            failed: Vec::new(),
        }
    }
    fn met(detail: impl Into<String>) -> Observation {
        Observation {
            met: true,
            settled: true,
            detail: detail.into(),
            failed: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub condition: String,
    pub met: bool,
    pub timed_out: bool,
    pub elapsed_secs: u64,
    pub looks: u32,
    pub last: Observation,
}

/// Looks every `every` until the condition holds (or, with `negate`, stops
/// holding), it settles as failed, or `timeout` passes. `progress` is called
/// when what's seen changes.
pub fn wait(
    cond: &Condition,
    negate: bool,
    timeout: Duration,
    every: Duration,
    ctx: &Ctx,
    mut progress: impl FnMut(&Observation, Duration),
) -> Outcome {
    let start = Instant::now();
    let mut looks = 0;
    let mut last_detail = String::new();
    loop {
        looks += 1;
        let mut seen = look(cond, ctx, start.elapsed());
        if negate {
            seen = Observation {
                met: !seen.met && !seen.settled,
                settled: !seen.met && !seen.settled,
                ..seen
            };
        }
        if seen.detail != last_detail {
            progress(&seen, start.elapsed());
            last_detail.clone_from(&seen.detail);
        }
        let elapsed = start.elapsed();
        if seen.met || seen.settled || elapsed >= timeout {
            let timed_out = !seen.met && !seen.settled;
            return Outcome {
                condition: format!("{}{}", if negate { "not: " } else { "" }, cond.describe()),
                met: seen.met,
                timed_out,
                elapsed_secs: elapsed.as_secs(),
                looks,
                last: seen,
            };
        }
        // Sleep until the next look, or the deadline for one last look.
        std::thread::sleep(every.min(timeout.saturating_sub(elapsed)));
    }
}

fn look(cond: &Condition, ctx: &Ctx, elapsed: Duration) -> Observation {
    match cond {
        Condition::Cmd(argv) => {
            let mut cmd = if argv.len() == 1 {
                let mut c = Command::new("sh");
                c.args(["-c", &argv[0]]);
                c
            } else {
                let mut c = Command::new(&argv[0]);
                c.args(&argv[1..]);
                c
            };
            cmd.current_dir(&ctx.cwd);
            match run(cmd, Duration::from_secs(300)) {
                Ok(o) if o.code == Some(0) => Observation::met("exited 0"),
                Ok(o) => {
                    let why = o.stderr.trim().lines().last().unwrap_or("").to_owned();
                    let code = o.code.map_or("by signal".to_owned(), |c| c.to_string());
                    if why.is_empty() {
                        Observation::pending(format!("exited {code}"))
                    } else {
                        Observation::pending(format!("exited {code}: {why}"))
                    }
                }
                Err(e) => Observation::pending(e),
            }
        }
        Condition::Port(addr) => {
            let addrs: Vec<_> = match addr.to_socket_addrs() {
                Ok(a) => a.collect(),
                Err(e) => return Observation::pending(format!("can't resolve {addr}: {e}")),
            };
            match addrs
                .iter()
                .find_map(|a| TcpStream::connect_timeout(a, Duration::from_secs(3)).ok())
            {
                Some(_) => Observation::met(format!("{addr} accepts connections")),
                None => Observation::pending(format!("{addr} refuses or doesn't answer")),
            }
        }
        Condition::Url { url, status } => {
            let mut cmd = Command::new("curl");
            cmd.args([
                "-sS",
                "-o",
                "/dev/null",
                "--max-time",
                "10",
                "-w",
                "%{http_code}",
                url,
            ]);
            match run(cmd, Duration::from_secs(20)) {
                Ok(o) => {
                    let code: u16 = o.stdout.trim().parse().unwrap_or(0);
                    let ok = match status {
                        Some(s) => code == *s,
                        None => (200..400).contains(&code),
                    };
                    let detail = if code == 0 {
                        o.stderr
                            .trim()
                            .lines()
                            .next()
                            .unwrap_or("no response")
                            .trim_start_matches("curl: ")
                            .to_owned()
                    } else {
                        format!("answered {code}")
                    };
                    if ok {
                        Observation::met(detail)
                    } else {
                        Observation::pending(detail)
                    }
                }
                Err(e) => Observation::pending(e),
            }
        }
        Condition::File(p) => {
            if p.exists() {
                Observation::met(format!("{} exists", p.display()))
            } else {
                Observation::pending(format!("{} doesn't exist", p.display()))
            }
        }
        Condition::Ci { what, repo } => ci(what.as_deref(), repo.as_deref(), ctx, elapsed),
    }
}

/// How long CI may report nothing before we decide there's none coming.
const NO_CI_GRACE: Duration = Duration::from_secs(120);

fn ci(what: Option<&str>, repo: Option<&str>, ctx: &Ctx, elapsed: Duration) -> Observation {
    let repo = match repo.map(str::to_owned).or_else(|| default_repo(ctx)) {
        Some(r) => r,
        None => {
            return settled_fail("no git remote here to find CI for; pass --repo host/owner/repo");
        }
    };
    let host = repo.split('/').next().unwrap_or("");
    if host.contains("gitlab") {
        return settled_fail(
            "GitLab CI isn't supported yet; wait on your own check instead: yuzu wait cmd -- '<command that exits 0 when done>'",
        );
    }
    let mut cmd = Command::new("gh");
    let pr = what.filter(|w| w.chars().all(|c| c.is_ascii_digit()));
    if let Some(n) = pr {
        cmd.args(["pr", "checks", n, "-R", &repo, "--json", "name,bucket,link"]);
    } else {
        let sha = git_out(&ctx.cwd, &["rev-parse", what.unwrap_or("HEAD")]);
        let Some(sha) = sha else {
            return settled_fail(&format!(
                "{} isn't a commit, branch, or PR number here",
                what.unwrap_or("HEAD")
            ));
        };
        cmd.args([
            "run",
            "list",
            "-R",
            &repo,
            "--commit",
            &sha,
            "--json",
            "name,status,conclusion,url",
            "-L",
            "50",
        ]);
    }
    cmd.envs(&ctx.vars).current_dir(&ctx.cwd);
    let out = match run(cmd, Duration::from_secs(30)) {
        Ok(o) => o,
        Err(e) => return Observation::pending(e),
    };
    // gh exits nonzero for pending and failing checks; the JSON still comes.
    let items: Vec<serde_json::Value> = match serde_json::from_str(out.stdout.trim()) {
        Ok(v) => v,
        Err(_) => {
            let err = out
                .stderr
                .trim()
                .lines()
                .next()
                .unwrap_or("no output")
                .to_owned();
            if err.contains("no checks reported") {
                Vec::new()
            } else if err.contains("401")
                || err.contains("Bad credentials")
                || err.contains("gh auth login")
            {
                // Waiting won't fix credentials.
                return settled_fail(&format!(
                    "gh can't authenticate ({err}). `yuzu whoami --verify` shows which login or token it's using."
                ));
            } else {
                return Observation::pending(format!("gh: {err}"));
            }
        }
    };
    let checks: Vec<Check> = items
        .iter()
        .map(|i| {
            let s = |k: &str| i.get(k).and_then(|v| v.as_str()).unwrap_or("").to_owned();
            if pr.is_some() {
                Check {
                    name: s("name"),
                    state: match s("bucket").as_str() {
                        "pass" | "skipping" => State::Passed,
                        "fail" | "cancel" => State::Failed,
                        _ => State::Pending,
                    },
                    link: s("link"),
                }
            } else {
                Check {
                    name: s("name"),
                    state: match (s("status").as_str(), s("conclusion").as_str()) {
                        ("completed", "success" | "skipped" | "neutral") => State::Passed,
                        ("completed", _) => State::Failed,
                        _ => State::Pending,
                    },
                    link: s("url"),
                }
            }
        })
        .collect();
    summarize(&checks, elapsed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Pending,
    Passed,
    Failed,
}

#[derive(Debug, Clone)]
struct Check {
    name: String,
    state: State,
    link: String,
}

fn summarize(checks: &[Check], elapsed: Duration) -> Observation {
    if checks.is_empty() {
        return if elapsed > NO_CI_GRACE {
            settled_fail("no CI checks or runs reported after 2 minutes; is CI set up for this?")
        } else {
            Observation::pending("no CI reported yet")
        };
    }
    let failed: Vec<&Check> = checks.iter().filter(|c| c.state == State::Failed).collect();
    let pending: Vec<&Check> = checks
        .iter()
        .filter(|c| c.state == State::Pending)
        .collect();
    if !pending.is_empty() {
        // Sorted, so a provider listing checks in a new order isn't a change.
        let mut names: Vec<&str> = pending.iter().map(|c| c.name.as_str()).collect();
        names.sort_unstable();
        return Observation::pending(format!(
            "{} of {} pending ({})",
            pending.len(),
            checks.len(),
            names.join(", ")
        ));
    }
    if failed.is_empty() {
        return Observation::met(format!("all {} passed", checks.len()));
    }
    Observation {
        met: false,
        settled: true,
        detail: format!("{} of {} failed", failed.len(), checks.len()),
        failed: failed
            .iter()
            .map(|c| format!("{}: {}", c.name, c.link))
            .collect(),
    }
}

fn settled_fail(detail: &str) -> Observation {
    Observation {
        met: false,
        settled: true,
        detail: detail.to_owned(),
        failed: Vec::new(),
    }
}

/// `host/owner/repo` for the remote a push would go to, or the first one.
fn default_repo(ctx: &Ctx) -> Option<String> {
    let branch = git_out(&ctx.cwd, &["symbolic-ref", "--short", "-q", "HEAD"]);
    let configured = branch.and_then(|b| {
        git_out(
            &ctx.cwd,
            &["config", "--get", &format!("branch.{b}.remote")],
        )
    });
    let names = git_out(&ctx.cwd, &["remote"])?;
    let remote = configured
        .filter(|r| names.lines().any(|n| n == r))
        .or_else(|| names.lines().find(|n| *n == "origin").map(str::to_owned))
        .or_else(|| names.lines().next().map(str::to_owned))?;
    remote_url(&ctx.cwd, &remote).map(|u| normalize_remote(&u))
}

/// `90`, `90s`, `10m`, `1h`.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let (num, unit) = s
        .find(|c: char| !c.is_ascii_digit())
        .map_or((s, ""), |i| (&s[..i], &s[i..]));
    let n: u64 = num
        .parse()
        .map_err(|_| format!("{s:?} isn't a duration: write 90s, 10m, or 1h"))?;
    let secs = match unit {
        "" | "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        _ => return Err(format!("{s:?} isn't a duration: write 90s, 10m, or 1h")),
    };
    Ok(Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(name: &str, state: State) -> Check {
        Check {
            name: name.into(),
            state,
            link: format!("https://ci/{name}"),
        }
    }

    #[test]
    fn summarizes_ci() {
        let short = Duration::from_secs(5);
        let o = summarize(
            &[
                check("linux", State::Passed),
                check("macos", State::Pending),
            ],
            short,
        );
        assert!(!o.met && !o.settled);
        assert_eq!(o.detail, "1 of 2 pending (macos)");
        let a = summarize(
            &[check("b", State::Pending), check("a", State::Pending)],
            short,
        );
        let b = summarize(
            &[check("a", State::Pending), check("b", State::Pending)],
            short,
        );
        assert_eq!(a.detail, b.detail);
        let o = summarize(
            &[check("linux", State::Passed), check("macos", State::Passed)],
            short,
        );
        assert!(o.met);
        let o = summarize(
            &[check("linux", State::Failed), check("macos", State::Passed)],
            short,
        );
        assert!(!o.met && o.settled);
        assert_eq!(o.failed, ["linux: https://ci/linux"]);
        // Nothing reported: keep waiting a while, then give up.
        assert!(!summarize(&[], short).settled);
        assert!(summarize(&[], Duration::from_secs(300)).settled);
    }

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("90").unwrap(), Duration::from_secs(90));
        assert_eq!(parse_duration("10m").unwrap(), Duration::from_secs(600));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert!(parse_duration("soon").is_err());
        assert!(parse_duration("5d").is_err());
    }

    #[test]
    fn waits_for_a_file_and_stops_at_the_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ready");
        let cond = Condition::File(path.clone());
        let ctx = Ctx::default();
        let tick = Duration::from_millis(20);
        let o = wait(
            &cond,
            false,
            Duration::from_millis(100),
            tick,
            &ctx,
            |_, _| {},
        );
        assert!(!o.met && o.timed_out);
        std::fs::write(&path, "").unwrap();
        let o = wait(&cond, false, Duration::from_secs(5), tick, &ctx, |_, _| {});
        assert!(o.met && !o.timed_out);
        assert_eq!(o.looks, 1);
        // --not: until it's gone.
        let o = wait(
            &cond,
            true,
            Duration::from_millis(100),
            tick,
            &ctx,
            |_, _| {},
        );
        assert!(!o.met && o.timed_out);
    }
}
