//! yuzu: what your environment is. Shows and classifies the targets a command
//! would touch (`status`, `check`) and checks services from where the user is
//! (`probe`). See docs/yuzu/.

mod config;
mod env;
mod mcp_tools;
mod policy;
mod probe;
mod sessions;
mod shell;
mod status;
mod targets;
mod wait;
mod whoami;

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use mochi_common::{Format, exit, print_json};
use serde::Serialize;

use crate::config::{Agents, Config};
use crate::env::Ctx;
use crate::policy::{Check, Decision};
use crate::targets::Access;

const AGENT_GUIDE: &str = include_str!("../AGENT_GUIDE.md");

#[derive(Debug, Parser)]
#[command(
    name = "yuzu",
    version,
    about = "What your environment is, before you act on it."
)]
struct Cli {
    /// Print one JSON object on stdout instead of text.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show the current contexts, profiles, and remotes, and whose they are. The default.
    Status,
    /// Classify what a command would touch, and block it if it isn't allowed.
    Check {
        /// The command to check, after `--`. One argument is read as a shell line.
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Run `check` as a hook: read the hook's JSON on stdin, answer on stdout.
    Hook {
        #[arg(value_enum)]
        agent: HookAgent,
    },
    /// Check a URL the way the user's browser would, and report the exact failure.
    Probe {
        /// The http or https URL the user opens.
        url: String,
        /// The browser to probe with (Chrome, Edge, Brave, or Chromium).
        /// Default: $YUZU_BROWSER, else the first one installed.
        #[arg(long, value_name = "PATH")]
        browser: Option<String>,
        /// Seconds to wait for each load.
        #[arg(long, default_value_t = 15, value_name = "SECS")]
        timeout: u64,
    },
    /// Which account each tool would act as here: gh, git, ssh, aws.
    Whoami {
        /// Ask each service who you are (network calls to GitHub, your git
        /// hosts, and AWS).
        #[arg(long)]
        verify: bool,
    },
    /// Wait until something is true: CI finishes, a port opens, a URL
    /// answers, a file appears, or a command succeeds.
    Wait {
        #[command(subcommand)]
        condition: WaitCondition,
        /// Give up after this long: 90s, 10m, 1h.
        #[arg(long, global = true, default_value = "10m", value_name = "DURATION")]
        timeout: String,
        /// How often to look. Default: 5s, 10s for url, 30s for ci.
        #[arg(long, global = true, value_name = "DURATION")]
        every: Option<String>,
        /// Wait until it's no longer true (the port closes, the file goes).
        #[arg(long, global = true)]
        not: bool,
    },
    /// How agents used tools, from their transcripts here, and what the
    /// hook asked or denied.
    Sessions {
        /// A CLI or MCP server name to count. Repeatable. Default: matcha, yuzu.
        #[arg(long = "tool", value_name = "NAME")]
        tools: Vec<String>,
        /// Only sessions this recent: 14d, 24h.
        #[arg(long, default_value = "14d", value_name = "DURATION")]
        since: String,
        /// Only sessions that ran in this directory or below it.
        #[arg(long, value_name = "PATH")]
        project: Option<String>,
        /// Leave out sessions in this directory or below, such as the
        /// tool's own repo while you build it. Repeatable.
        #[arg(long = "exclude", value_name = "PATH")]
        exclude: Vec<String>,
        /// A transcript folder to read instead of finding them. Repeatable.
        #[arg(long = "dir", value_name = "PATH")]
        dirs: Vec<String>,
    },
    /// Serve status, check, probe, whoami, and wait as MCP tools on stdin and stdout.
    Mcp,
    /// Print a short guide to yuzu, written for AI agents.
    AgentGuide,
}

#[derive(Debug, Subcommand)]
enum WaitCondition {
    /// Until CI for a PR number, branch, tag, or commit (default HEAD)
    /// finishes. Exit 0 if it passed, 1 if it failed.
    Ci {
        /// A PR number, or a commit, branch, or tag.
        what: Option<String>,
        /// host/owner/repo. Default: from this repo's remote.
        #[arg(long, value_name = "HOST/OWNER/REPO")]
        repo: Option<String>,
    },
    /// Until host:port accepts a TCP connection.
    Port {
        /// host:port, such as 127.0.0.1:8200.
        address: String,
    },
    /// Until the URL answers 2xx or 3xx, or --status.
    Url {
        /// The http or https URL.
        url: String,
        /// Wait for exactly this status code instead.
        #[arg(long)]
        status: Option<u16>,
    },
    /// Until the path exists.
    File {
        /// The file or directory.
        path: String,
    },
    /// Until the command exits 0. One argument runs through `sh -c`.
    Cmd {
        /// The command, after `--`.
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum HookAgent {
    /// A Claude Code `PreToolUse` hook on Bash.
    ClaudeCode,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = Format::from_json_flag(cli.json);
    let ctx = Ctx::from_process();

    let code = match cli.command.unwrap_or(Command::Status) {
        Command::AgentGuide => {
            print!("{AGENT_GUIDE}");
            exit::OK
        }
        Command::Status => run_status(&ctx, format),
        Command::Mcp => mcp_tools::serve(),
        Command::Whoami { verify } => run_whoami(&ctx, verify, format),
        Command::Sessions {
            tools,
            since,
            project,
            exclude,
            dirs,
        } => run_sessions(
            &ctx,
            tools,
            &since,
            project.as_deref(),
            &exclude,
            &dirs,
            format,
        ),
        Command::Wait {
            condition,
            timeout,
            every,
            not,
        } => run_wait(&ctx, condition, &timeout, every.as_deref(), not, format),
        Command::Check { command } => run_check(&ctx, &command, format),
        Command::Hook {
            agent: HookAgent::ClaudeCode,
        } => run_claude_hook(ctx),
        Command::Probe {
            url,
            browser,
            timeout,
        } => run_probe(&ctx, &url, browser.as_deref(), timeout, format),
    };
    ExitCode::from(code)
}

fn run_wait(
    ctx: &Ctx,
    condition: WaitCondition,
    timeout: &str,
    every: Option<&str>,
    not: bool,
    format: Format,
) -> u8 {
    let cond = match condition {
        WaitCondition::Ci { what, repo } => wait::Condition::Ci { what, repo },
        WaitCondition::Port { address } => wait::Condition::Port(address),
        WaitCondition::Url { url, status } => wait::Condition::Url { url, status },
        WaitCondition::File { path } => wait::Condition::File(ctx.path(&path)),
        WaitCondition::Cmd { command } => wait::Condition::Cmd(command),
    };
    let durations = wait::parse_duration(timeout).and_then(|t| {
        every
            .map(wait::parse_duration)
            .transpose()
            .map(|e| (t, e.unwrap_or_else(|| cond.default_every())))
    });
    let (timeout, every) = match durations {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return exit::USAGE;
        }
    };
    eprintln!(
        "Waiting up to {} for {}{}.",
        clock(timeout),
        if not { "not: " } else { "" },
        cond.describe()
    );
    let outcome = wait::wait(&cond, not, timeout, every, ctx, |seen, at| {
        eprintln!("  [{}] {}", clock(at), seen.detail);
    });
    match format {
        Format::Json => print_json(&outcome),
        Format::Text => {
            let verdict = if outcome.met {
                "Done"
            } else if outcome.timed_out {
                "Timed out"
            } else {
                "Failed"
            };
            println!(
                "{verdict} after {} ({} looks): {}",
                clock(std::time::Duration::from_secs(outcome.elapsed_secs)),
                outcome.looks,
                outcome.last.detail
            );
            for f in &outcome.last.failed {
                println!("  {f}");
            }
            if outcome.timed_out {
                println!("  Still not done. Wait longer with --timeout, or check why it's stuck.");
            }
        }
    }
    if outcome.met {
        exit::OK
    } else {
        exit::CHECK_FAILED
    }
}

/// `4m12s`, `35s`, `1h2m`.
fn clock(d: std::time::Duration) -> String {
    let s = d.as_secs();
    match (s / 3600, s % 3600 / 60, s % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, s) => format!("{m}m{s:02}s"),
        (h, m, _) => format!("{h}h{m:02}m"),
    }
}

fn run_sessions(
    ctx: &Ctx,
    mut tools: Vec<String>,
    since: &str,
    project: Option<&str>,
    exclude: &[String],
    dirs: &[String],
    format: Format,
) -> u8 {
    let since = match parse_age(since) {
        Ok(secs) => {
            mochi_common::time::rfc3339(mochi_common::time::unix_now().saturating_sub(secs))
        }
        Err(e) => {
            eprintln!("{e}");
            return exit::USAGE;
        }
    };
    if tools.is_empty() {
        tools = vec!["matcha".into(), "yuzu".into()];
    }
    let dirs: Vec<std::path::PathBuf> = if dirs.is_empty() {
        sessions::transcript_dirs(ctx)
    } else {
        let mut given: Vec<std::path::PathBuf> = dirs
            .iter()
            .map(|d| ctx.path(d))
            .map(|d| d.canonicalize().unwrap_or(d))
            .collect();
        given.sort();
        given.dedup();
        given
    };
    if dirs.is_empty() {
        eprintln!(
            "No agent transcripts found (looked for ~/.claude*/projects). Pass --dir <path>."
        );
        return exit::USAGE;
    }
    let project = project.map(|p| ctx.path(p));
    let exclude: Vec<std::path::PathBuf> = exclude.iter().map(|p| ctx.path(p)).collect();
    let r = sessions::report(ctx, &dirs, &tools, &since, project.as_deref(), &exclude);
    match format {
        Format::Json => print_json(&r),
        Format::Text => {
            println!(
                "{} sessions since {} in {} transcript {}.",
                r.sessions,
                &r.since[..10],
                r.transcript_dirs.len(),
                if r.transcript_dirs.len() == 1 {
                    "folder"
                } else {
                    "folders"
                }
            );
            for u in &r.tools {
                if u.calls == 0 {
                    println!("  {:<8} not used", u.tool);
                    continue;
                }
                println!(
                    "  {:<8} used in {} of {} sessions, {} calls ({} unprompted, in {} {})",
                    u.tool,
                    u.sessions_using,
                    r.sessions,
                    u.calls,
                    u.unprompted_calls,
                    u.unprompted_sessions,
                    if u.unprompted_sessions == 1 {
                        "session"
                    } else {
                        "sessions"
                    }
                );
                let mut by: Vec<(&String, &usize)> = u.by_command.iter().collect();
                by.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
                let list: Vec<String> = by.iter().map(|(c, n)| format!("{c} {n}")).collect();
                println!("           {}", list.join(", "));
                let mut projects: Vec<(&String, &usize)> = u.by_project.iter().collect();
                projects.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
                let list: Vec<String> = projects.iter().map(|(p, n)| format!("{p} {n}")).collect();
                println!("           in: {}", list.join(", "));
                for e in &u.examples {
                    println!(
                        "           e.g. {}",
                        e.chars().take(100).collect::<String>()
                    );
                }
            }
            match &r.hook {
                Some(h) => {
                    println!(
                        "yuzu hook: {} asks, {} denies since {} ({})",
                        h.asks,
                        h.denies,
                        &r.since[..10],
                        h.log
                    );
                    for e in &h.recent {
                        println!(
                            "  {}  {:<4}  {}",
                            &e.at[..16],
                            e.decision,
                            e.command.chars().take(80).collect::<String>()
                        );
                    }
                    if !h.recent.is_empty() {
                        println!("  Review these: any that should have run is a wrong block.");
                    }
                }
                None => println!(
                    "yuzu hook: no decisions logged (the hook isn't installed, or it hasn't asked yet)."
                ),
            }
        }
    }
    exit::OK
}

/// `14d`, `24h`, `90m`, or seconds.
fn parse_age(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let (num, unit) = s
        .find(|c: char| !c.is_ascii_digit())
        .map_or((s, ""), |i| (&s[..i], &s[i..]));
    let n: u64 = num
        .parse()
        .map_err(|_| format!("{s:?} isn't an age: write 14d, 24h, or 90m"))?;
    match unit {
        "" | "s" => Ok(n),
        "m" => Ok(n * 60),
        "h" => Ok(n * 3600),
        "d" => Ok(n * 86_400),
        _ => Err(format!("{s:?} isn't an age: write 14d, 24h, or 90m")),
    }
}

fn run_whoami(ctx: &Ctx, verify: bool, format: Format) -> u8 {
    let ids = whoami::whoami(ctx, verify);
    let failed = ids
        .iter()
        .filter(|i| i.verified.as_ref().is_some_and(|v| !v.ok))
        .count();
    match format {
        Format::Json => print_json(
            &serde_json::json!({ "identities": ids, "verified": verify, "failed": failed }),
        ),
        Format::Text => {
            if ids.is_empty() {
                println!("No gh login, git author, ssh remote, or AWS credentials found here.");
            }
            for i in &ids {
                println!("  {:<7} {}: {}   ({})", i.service, i.scope, i.who, i.source);
                if let Some(w) = &i.warning {
                    println!("          ! {w}");
                }
                if let Some(v) = &i.verified {
                    let mark = if v.ok { "ok" } else { "FAIL" };
                    println!("          {mark}: {}", v.detail);
                    if let Some(fix) = &v.fix {
                        println!("          fix: {fix}");
                    }
                }
            }
            if !verify && ids.iter().any(|i| i.service != "git") {
                println!("  (offline: --verify asks each service who you are)");
            }
        }
    }
    if failed > 0 {
        exit::CHECK_FAILED
    } else {
        exit::OK
    }
}

fn run_probe(ctx: &Ctx, url: &str, browser: Option<&str>, timeout: u64, format: Format) -> u8 {
    let target = match probe::parse_url(url) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            return exit::USAGE;
        }
    };
    let timeout = std::time::Duration::from_secs(timeout.max(1));
    let dns = probe::resolve(&target.host, target.port);
    let terminal = probe::curl(&target, None, timeout);
    // With both address families, try each, so a dead AAAA record shows.
    let (v4, v6) = if !dns.v4.is_empty() && !dns.v6.is_empty() {
        (
            Some(probe::curl(&target, Some(4), timeout)),
            Some(probe::curl(&target, Some(6), timeout)),
        )
    } else {
        (None, None)
    };
    let bin = probe::find_browser(browser, ctx);
    let load = bin
        .as_ref()
        .map(|b| probe::browser_load(b, &target, timeout));
    let proxies = probe::proxies(ctx);
    let ca_env: Vec<String> = ["CURL_CA_BUNDLE", "SSL_CERT_FILE", "SSL_CERT_DIR"]
        .iter()
        .filter_map(|k| ctx.var(k).map(|v| format!("{k}={v}")))
        .collect();
    let diagnosis = probe::diagnose(&probe::Facts {
        target: &target,
        dns: &dns,
        terminal: &terminal,
        terminal_v4: v4.as_ref(),
        terminal_v6: v6.as_ref(),
        browser: load.as_ref(),
        proxies: &proxies,
        macos: cfg!(target_os = "macos"),
        ca_env,
        now: mochi_common::time::unix_now(),
    });
    let ok = terminal.ok && load.as_ref().is_some_and(|l| l.attempt.ok);

    match format {
        Format::Json => print_json(&serde_json::json!({
            "url": target.url,
            "host": target.host,
            "port": target.port,
            "ok": ok,
            "dns": dns,
            "terminal": terminal,
            "terminal_ipv4": v4,
            "terminal_ipv6": v6,
            "browser": load,
            "browser_found": bin.is_some(),
            "proxies": proxies,
            "diagnosis": diagnosis,
        })),
        Format::Text => {
            let addrs: Vec<&str> = dns.v4.iter().chain(&dns.v6).map(String::as_str).collect();
            let dns_line = match (&dns.error, addrs.is_empty()) {
                (Some(e), _) => format!("FAIL  {e}"),
                (None, true) => "no addresses".to_owned(),
                (None, false) => addrs.join(", "),
            };
            let row = |label: &str, value: &str| println!("  {label:<30}  {value}");
            row(&format!("dns ({})", target.host), &dns_line);
            row("terminal (curl)", &describe(&terminal));
            for (label, a) in [("terminal over IPv4", &v4), ("terminal over IPv6", &v6)] {
                if let Some(a) = a {
                    row(label, &describe(a));
                }
            }
            match &load {
                Some(l) => {
                    let label = match &l.version {
                        Some(v) => format!("{} {v}", l.name),
                        None => l.name.clone(),
                    };
                    row(&label, &describe(&l.attempt));
                }
                None => row(
                    "browser",
                    "not found: install Chrome, or pass --browser <path> (Edge, Brave, and Chromium work too)",
                ),
            }
            for d in &diagnosis {
                println!("  likely cause: {}", d.cause);
                println!("    {}", d.evidence);
                println!("    fix: {}", d.fix);
            }
            if !ok && diagnosis.is_empty() {
                println!(
                    "  No rule matches this combination. The lines above are everything yuzu saw; it won't guess."
                );
            }
        }
    }
    if ok { exit::OK } else { exit::CHECK_FAILED }
}

/// `200   10.0.0.8   cert trusted` or `FAIL  ERR_ADDRESS_UNREACHABLE`.
fn describe(a: &probe::Attempt) -> String {
    if !a.ok {
        return format!("FAIL  {}", a.error.as_deref().unwrap_or("no response"));
    }
    let mut parts = vec![a.status.map_or("loaded".to_owned(), |s| s.to_string())];
    if let Some(ip) = &a.ip {
        parts.push(ip.clone());
    }
    match a.cert_trusted {
        Some(true) => parts.push("cert trusted".into()),
        Some(false) => parts.push("cert NOT trusted".into()),
        None => {}
    }
    parts.join("   ")
}

fn run_status(ctx: &Ctx, format: Format) -> u8 {
    let config = match Config::load(&Config::path(ctx)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}. Fix the file, then run yuzu again.");
            return exit::USAGE;
        }
    };
    let s = status::status(&config, ctx);
    match format {
        Format::Json => print_json(&s),
        Format::Text => {
            if !config.found {
                println!(
                    "No environments declared. Write them in {} (see `yuzu agent-guide`).",
                    config.path.display()
                );
            }
            if s.targets.is_empty() {
                println!("No kube context, aws profile, or git remote is set here.");
            }
            for t in &s.targets {
                let whose = match &t.env {
                    Some(env) => format!("{env} ({}, agents {})", t.class, agents_label(t.agents)),
                    None => "unknown (not declared)".to_owned(),
                };
                println!("  {:<13} {:<44} {}", t.kind.label(), t.name, whose);
            }
        }
    }
    exit::OK
}

fn agents_label(a: Agents) -> &'static str {
    match a {
        Agents::Write => "write",
        Agents::ReadOnly => "read-only",
        Agents::None => "none",
    }
}

/// Classifies `words` and decides. A config that won't load blocks, so a
/// broken file can't quietly allow everything.
fn decide(ctx: &Ctx, words: &[String]) -> Check {
    let config = match Config::load(&Config::path(ctx)) {
        Ok(c) => c,
        Err(e) => {
            return Check {
                decision: Decision::Ask,
                targets: Vec::new(),
                message: Some(format!(
                    "yuzu can't check this command: {e}. Ask the user to fix the file."
                )),
            };
        }
    };
    let found = match words {
        [line] => targets::find(line, ctx),
        _ => targets::find_words(words, ctx),
    };
    policy::check(&config, found)
}

fn run_check(ctx: &Ctx, words: &[String], format: Format) -> u8 {
    let c = decide(ctx, words);
    match format {
        Format::Json => print_json(&c),
        Format::Text => {
            for v in &c.targets {
                let whose = v
                    .env
                    .as_ref()
                    .map_or_else(|| "unknown".to_owned(), |e| format!("{e} ({})", v.class));
                let access = match v.target.access {
                    Access::Read => "read",
                    Access::Write => "write",
                };
                println!(
                    "  {:<13} {:<44} {:<24} {access}",
                    v.target.kind.label(),
                    v.target.name,
                    whose
                );
            }
            match c.decision {
                Decision::Allow if c.targets.is_empty() => {
                    println!(
                        "Allowed: it doesn't reach a cluster, host, remote, or account yuzu knows."
                    );
                }
                Decision::Allow => println!("Allowed."),
                Decision::Ask => println!("NEEDS APPROVAL"),
                Decision::Deny => println!("DENIED"),
            }
            if let Some(m) = &c.message {
                println!("{m}");
            }
        }
    }
    if c.decision == Decision::Allow {
        exit::OK
    } else {
        exit::CHECK_FAILED
    }
}

/// What Claude Code sends a `PreToolUse` hook (only the fields yuzu reads).
#[derive(Debug, serde::Deserialize)]
struct HookInput {
    tool_name: String,
    #[serde(default)]
    tool_input: serde_json::Value,
    #[serde(default)]
    cwd: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HookOutput {
    hook_specific_output: HookDecision,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HookDecision {
    hook_event_name: &'static str,
    permission_decision: &'static str,
    permission_decision_reason: String,
}

/// Says nothing when the command is allowed, so Claude Code's own permission
/// rules still apply; answers `ask` or `deny` otherwise. Always exits 0: the
/// answer is in the JSON.
fn run_claude_hook(mut ctx: Ctx) -> u8 {
    let mut stdin = String::new();
    let input: Result<HookInput, String> = std::io::stdin()
        .read_to_string(&mut stdin)
        .map_err(|e| e.to_string())
        .and_then(|_| serde_json::from_str(&stdin).map_err(|e| e.to_string()));
    let mut logged_command = String::new();
    let (decision, reason) = match input {
        Err(e) => (
            Decision::Ask,
            format!("yuzu couldn't read the hook input ({e}), so it can't check this command."),
        ),
        Ok(input) if input.tool_name != "Bash" => return exit::OK,
        Ok(input) => {
            let Some(command) = input.tool_input.get("command").and_then(|c| c.as_str()) else {
                return exit::OK;
            };
            if let Some(cwd) = input.cwd {
                ctx.cwd = cwd;
            }
            command.clone_into(&mut logged_command);
            let c = decide(&ctx, &[command.to_owned()]);
            (c.decision, c.message.unwrap_or_default())
        }
    };
    let permission_decision = match decision {
        Decision::Allow => return exit::OK,
        Decision::Ask => "ask",
        Decision::Deny => "deny",
    };
    // Asks and denies are kept, so the trial can count wrong blocks.
    sessions::log_decision(
        &ctx,
        &sessions::Logged {
            at: mochi_common::time::now_rfc3339(),
            decision: permission_decision.to_owned(),
            command: logged_command,
            cwd: ctx.cwd.display().to_string(),
            reason: reason.lines().next().unwrap_or("").to_owned(),
        },
    );
    print_json(&HookOutput {
        hook_specific_output: HookDecision {
            hook_event_name: "PreToolUse",
            permission_decision,
            permission_decision_reason: format!("yuzu: {reason}"),
        },
    });
    exit::OK
}
