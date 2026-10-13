//! yuzu: what your environment is. Shows and classifies the targets a command
//! would touch (`status`, `check`) and checks services from where the user is
//! (`probe`). See docs/design.md.

mod config;
mod env;
mod mcp_tools;
mod policy;
mod probe;
mod proc;
mod shell;
mod status;
mod targets;
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
    /// Serve status, check, and probe as MCP tools on stdin and stdout.
    Mcp,
    /// Print a short guide to yuzu, written for AI agents.
    AgentGuide,
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
            let c = decide(&ctx, &[command.to_owned()]);
            (c.decision, c.message.unwrap_or_default())
        }
    };
    let permission_decision = match decision {
        Decision::Allow => return exit::OK,
        Decision::Ask => "ask",
        Decision::Deny => "deny",
    };
    print_json(&HookOutput {
        hook_specific_output: HookDecision {
            hook_event_name: "PreToolUse",
            permission_decision,
            permission_decision_reason: format!("yuzu: {reason}"),
        },
    });
    exit::OK
}
