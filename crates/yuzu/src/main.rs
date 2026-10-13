//! yuzu: what your environment is. Shows and classifies the targets a command
//! would touch (`status`, `check`) and checks services from where the user is
//! (`probe`). See docs/design.md.

mod config;
mod env;
mod policy;
mod shell;
mod status;
mod targets;

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use mochi_common::{Format, exit, not_implemented, print_json};
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
    Probe { url: String },
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
        Command::Check { command } => run_check(&ctx, &command, format),
        Command::Hook {
            agent: HookAgent::ClaudeCode,
        } => run_claude_hook(ctx),
        Command::Probe { .. } => not_implemented(format, "yuzu", "probe"),
    };
    ExitCode::from(code)
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
