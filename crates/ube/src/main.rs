//! ube: the change in hand. Which of the repo's checks apply to it (`plan`),
//! run them and report failures in a few lines (`run`), whether it's done
//! with evidence (`done`), and whether its tests can fail (`mutate`).
//! See docs/ube/.

mod detect;
mod done;
mod hook;
mod mcp_tools;
mod mutate;
mod parse;
mod plan;
mod run;
mod store;

use std::path::PathBuf;
use std::process::{Command as Process, ExitCode};
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use mochi_common::{Format, exit, print_json};
use mochi_index::RepoRoot;

const AGENT_GUIDE: &str = include_str!("../AGENT_GUIDE.md");

#[derive(Debug, Parser)]
#[command(
    name = "ube",
    version,
    about = "The change in hand: which checks apply, and whether they passed on this code."
)]
struct Cli {
    /// Print one JSON object on stdout instead of text.
    #[arg(long, global = true)]
    json: bool,

    /// The repo to look at. Defaults to the one containing the current directory.
    #[arg(long, global = true, value_name = "PATH")]
    repo: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Which of the repo's checks apply to this change, and why.
    Plan(Scope),
    /// Run the planned checks; failures in a few lines; exit 1 if any.
    Run(RunArgs),
    /// Is this change done? A checklist for the exact current code.
    Done {
        /// Compare against this commit or branch instead of HEAD.
        #[arg(long, value_name = "REV")]
        base: Option<String>,
    },
    /// Mutate changed lines and run their tests: which changes go unnoticed?
    Mutate(MutateArgs),
    /// Run as an agent hook: read the hook's JSON on stdin.
    Hook {
        #[arg(value_enum)]
        agent: HookAgent,
    },
    /// Serve plan, run, and done as MCP tools on stdin and stdout.
    Mcp,
    /// Print a short guide to ube, written for AI agents.
    AgentGuide,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum HookAgent {
    /// A Claude Code `Stop` hook: asks the agent to prove its change before
    /// it stops.
    ClaudeCode,
}

#[derive(Debug, Args)]
struct Scope {
    /// Compare against this commit or branch instead of HEAD.
    #[arg(long, value_name = "REV")]
    base: Option<String>,
    /// Plan the full suite, not a selection.
    #[arg(long)]
    all: bool,
}

#[derive(Debug, Args)]
struct RunArgs {
    #[command(flatten)]
    scope: Scope,
    /// Rerun only the checks that failed last time on this code.
    #[arg(long)]
    rerun_failed: bool,
    /// Stop a check after this many seconds.
    #[arg(long, default_value_t = 1800, value_name = "SECS")]
    timeout: u64,
    /// Run this command instead of the plan, after `--`. One argument is a shell line.
    #[arg(last = true)]
    command: Vec<String>,
}

#[derive(Debug, Args)]
struct MutateArgs {
    /// Compare against this commit or branch instead of HEAD.
    #[arg(long, value_name = "REV")]
    base: Option<String>,
    /// Only mutate lines in these files or directories. Repeatable.
    #[arg(long = "path", value_name = "PATH")]
    paths: Vec<String>,
    /// Mutations to try at most, spread over the change.
    #[arg(long, default_value_t = 20)]
    max: usize,
    /// Stop a check after this many seconds.
    #[arg(long, default_value_t = 600, value_name = "SECS")]
    timeout: u64,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = Format::from_json_flag(cli.json);
    match cli.command {
        Command::AgentGuide => {
            print!("{AGENT_GUIDE}");
            return ExitCode::from(exit::OK);
        }
        Command::Mcp => return ExitCode::from(mcp_tools::serve()),
        Command::Hook {
            agent: HookAgent::ClaudeCode,
        } => {
            if let Some(reason) = hook::claude_code(stop_reason) {
                print_json(&serde_json::json!({ "decision": "block", "reason": reason }));
            }
            return ExitCode::from(exit::OK);
        }
        _ => {}
    }
    let start = cli.repo.unwrap_or_else(|| PathBuf::from("."));
    let root = match RepoRoot::discover(&start) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}. Run ube inside a git repo, or pass --repo <path>.");
            return ExitCode::from(exit::USAGE);
        }
    };
    let result = match cli.command {
        Command::Plan(scope) => cmd_plan(&root, &scope, format),
        Command::Run(args) => cmd_run(&root, &args, format),
        Command::Done { base } => cmd_done(&root, base.as_deref(), format),
        Command::Mutate(args) => cmd_mutate(&root, &args, format),
        Command::AgentGuide | Command::Mcp | Command::Hook { .. } => Ok(exit::OK),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(exit::USAGE)
        }
    }
}

fn err(e: mochi_index::IndexError) -> String {
    e.to_string()
}

/// Whether `matcha` is on PATH, for planning docs and recording evidence.
fn has_matcha() -> bool {
    Process::new("matcha")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The project, read from tracked files and new ones not yet added (a new
/// package counts before its first commit), and the changed files.
fn read_project(
    root: &RepoRoot,
    base: Option<&str>,
) -> Result<(detect::Project, Vec<String>), String> {
    let changed = root.changed_files(base).map_err(err)?;
    let mut files = root.tracked_files().map_err(err)?;
    files.extend(
        changed
            .iter()
            .filter(|f| root.path().join(f).is_file())
            .cloned(),
    );
    files.sort();
    files.dedup();
    Ok((detect::detect(root.path(), &files), changed))
}

fn make_plan(root: &RepoRoot, base: Option<&str>, all: bool) -> Result<plan::Plan, String> {
    let (project, changed) = read_project(root, base)?;
    Ok(plan::plan(&project, &changed, all, has_matcha()))
}

fn cmd_plan(root: &RepoRoot, scope: &Scope, format: Format) -> Result<u8, String> {
    let p = make_plan(root, scope.base.as_deref(), scope.all)?;
    match format {
        Format::Json => print_json(&p),
        Format::Text => {
            if p.changed.is_empty() && !scope.all {
                println!(
                    "No changes against {}: nothing to check. Use --all for the full suite.",
                    scope.base.as_deref().unwrap_or("HEAD")
                );
                return Ok(exit::OK);
            }
            if p.checks.is_empty() {
                println!(
                    "No checks found: the repo declares no tests, lint, or CI checks ube can read."
                );
            }
            for c in &p.checks {
                let at = if c.dir.is_empty() {
                    String::new()
                } else {
                    format!("  (in {})", c.dir)
                };
                println!("  {}{at}", c.command);
                println!("      {}", c.reason);
            }
            for l in &p.left_out {
                println!("  left out: {}  ({})", l.command, l.why);
            }
        }
    }
    Ok(exit::OK)
}

fn cmd_run(root: &RepoRoot, args: &RunArgs, format: Format) -> Result<u8, String> {
    let tree = root.worktree_tree().map_err(err)?;
    let store = store::Store::new(root.path());
    let mut checks = if args.command.is_empty() {
        make_plan(root, args.scope.base.as_deref(), args.scope.all)?.checks
    } else {
        vec![plan::Check {
            command: args.command.join(" "),
            dir: String::new(),
            reason: "given".into(),
            tests: true,
        }]
    };
    if args.rerun_failed {
        let last = store.load(&tree);
        checks.retain(|c| last.get(&c.id()).is_some_and(|r| !r.passed));
        if checks.is_empty() {
            if format == Format::Text {
                println!(
                    "Nothing failed on this code last time. Run without --rerun-failed to run the plan."
                );
            } else {
                print_json(&serde_json::json!({ "tree": tree, "results": [], "passed": true }));
            }
            return Ok(exit::OK);
        }
    }
    if checks.is_empty() {
        if format == Format::Text {
            println!(
                "No checks to run: no changes, or no checks the repo declares. Use --all, or -- <command>."
            );
        } else {
            print_json(&serde_json::json!({ "tree": tree, "results": [], "passed": true }));
        }
        return Ok(exit::OK);
    }
    let timeout = Duration::from_secs(args.timeout.max(1));
    let mut results = Vec::new();
    for c in &checks {
        eprintln!(
            "ube: running {}{}",
            c.command,
            if c.dir.is_empty() {
                String::new()
            } else {
                format!(" (in {})", c.dir)
            }
        );
        let r = run::run_check(root.path(), c, timeout, &[]);
        eprintln!(
            "ube: {} in {:.1}s",
            if r.passed { "passed" } else { "FAILED" },
            r.secs
        );
        results.push((c.id(), r));
    }
    let after = root.worktree_tree().map_err(err)?;
    store.save(&tree, &results)?;
    let passed = results.iter().all(|(_, r)| r.passed);
    let recorded = passed && record_evidence(root, &tree, &results);
    match format {
        Format::Json => print_json(&serde_json::json!({
            "tree": tree,
            "results": results.iter().map(|(_, r)| r).collect::<Vec<_>>(),
            "passed": passed,
            "tree_changed": after != tree,
            "recorded_in_ledger": recorded,
        })),
        Format::Text => {
            for (_, r) in &results {
                let at = if r.dir.is_empty() {
                    String::new()
                } else {
                    format!(" (in {})", r.dir)
                };
                println!(
                    "{}  {}{at}  {:.1}s",
                    if r.passed { "ok  " } else { "FAIL" },
                    r.command,
                    r.secs
                );
                for f in &r.failures {
                    let place = match (&f.file, f.line) {
                        (Some(file), Some(line)) => format!("{file}:{line}"),
                        (Some(file), None) => file.clone(),
                        _ => String::new(),
                    };
                    let test = f
                        .test
                        .as_deref()
                        .map(|t| format!("  {t}"))
                        .unwrap_or_default();
                    println!("      {place}{test}");
                    for l in f.message.lines().take(4) {
                        println!("        {l}");
                    }
                    if let Some(rerun) = &f.rerun {
                        println!("        rerun: {rerun}");
                    }
                }
                if !r.tail.is_empty() {
                    println!("      (no failure could be read from the output; its last lines:)");
                    for l in &r.tail {
                        println!("        {l}");
                    }
                }
            }
            let failed = results.iter().filter(|(_, r)| !r.passed).count();
            if failed == 0 {
                println!(
                    "All {} passed on tree {}.{}",
                    results.len(),
                    &tree[..12.min(tree.len())],
                    if recorded {
                        " Recorded in the ledger."
                    } else {
                        ""
                    }
                );
            } else {
                println!(
                    "{failed} of {} failed. ube run --rerun-failed reruns just those.",
                    results.len()
                );
            }
            if after != tree {
                println!(
                    "The checks changed files, so the code now differs from what they ran on. Run again to prove it."
                );
            }
        }
    }
    Ok(if passed { exit::OK } else { exit::CHECK_FAILED })
}

/// Adds a verified ledger entry for a green run, when matcha is installed.
fn record_evidence(root: &RepoRoot, tree: &str, results: &[(String, store::CheckResult)]) -> bool {
    if !has_matcha() {
        return false;
    }
    let commands: Vec<&str> = results.iter().map(|(_, r)| r.command.as_str()).collect();
    let claim = format!(
        "{} pass{} on tree {}",
        commands.join(", "),
        if commands.len() == 1 { "es" } else { "" },
        &tree[..12.min(tree.len())]
    );
    let evidence = results
        .iter()
        .map(|(_, r)| {
            format!(
                "{}: exit {} in {:.1}s",
                r.command,
                r.exit.unwrap_or(-1),
                r.secs
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    Process::new("matcha")
        .arg("--repo")
        .arg(root.path())
        .args(["ledger", "add", "verified", &claim, "--evidence", &evidence])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn cmd_done(root: &RepoRoot, base: Option<&str>, format: Format) -> Result<u8, String> {
    let (project, changed) = read_project(root, base)?;
    let p = plan::plan(&project, &changed, false, has_matcha());
    let tree = root.worktree_tree().map_err(err)?;
    let results = store::Store::new(root.path()).load(&tree);
    let added = root.added_lines(base).map_err(err)?;
    let inline = done::inline_tested(root.path(), &added);
    let items = done::done(root.path(), &project, &p, &results, base, &inline);
    let blocked = items.iter().any(|i| i.state == done::State::Fail);
    match format {
        Format::Json => {
            print_json(&serde_json::json!({ "tree": tree, "done": !blocked, "items": items }))
        }
        Format::Text => {
            for i in &items {
                let mark = match i.state {
                    done::State::Ok => "ok  ",
                    done::State::Warn => "warn",
                    done::State::Fail => "FAIL",
                    done::State::Unknown => "?   ",
                };
                println!("{mark}  {:<10} {}", i.name, i.detail);
                if let Some(fix) = &i.fix
                    && i.state != done::State::Ok
                {
                    println!("      {:<10} {fix}", "");
                }
            }
            println!(
                "{}",
                if blocked {
                    "Not done yet."
                } else {
                    "Done, as far as these checks can tell."
                }
            );
        }
    }
    Ok(if blocked {
        exit::CHECK_FAILED
    } else {
        exit::OK
    })
}

/// For the Stop hook: why the agent should keep going, if the change's
/// checks haven't passed on this exact code.
fn stop_reason(root: &RepoRoot) -> Option<String> {
    let (project, changed) = read_project(root, None).ok()?;
    if changed.is_empty() {
        return None;
    }
    let p = plan::plan(&project, &changed, false, false);
    if p.checks.is_empty() {
        return None;
    }
    let tree = root.worktree_tree().ok()?;
    let results = store::Store::new(root.path()).load(&tree);
    let pending: Vec<&str> = p
        .checks
        .iter()
        .filter(|c| !results.get(&c.id()).is_some_and(|r| r.passed))
        .map(|c| c.command.as_str())
        .collect();
    (!pending.is_empty()).then(|| {
        format!(
            "ube: this change isn't proven yet. These checks haven't passed on the current code: {}. Run `ube run` (or the ube run tool), or tell the user plainly what's unverified.",
            pending.join(", ")
        )
    })
}

fn cmd_mutate(root: &RepoRoot, args: &MutateArgs, format: Format) -> Result<u8, String> {
    let added = root.added_lines(args.base.as_deref()).map_err(err)?;
    let all = mutate::candidates(&added, &args.paths);
    let count = all.len();
    if all.is_empty() {
        if format == Format::Text {
            println!(
                "Nothing to mutate: no changed code lines with an operator or a boolean to flip."
            );
        } else {
            print_json(&serde_json::json!({ "candidates": 0, "mutants": [] }));
        }
        return Ok(exit::OK);
    }
    let p = make_plan(root, args.base.as_deref(), false)?;
    let checks: Vec<plan::Check> = p.checks.into_iter().filter(|c| c.tests).collect();
    if checks.is_empty() {
        return Err("No test checks apply to this change, so there's nothing to run against mutations. See ube plan.".into());
    }
    let picked = mutate::spread(all, args.max);
    eprintln!(
        "ube: {} of {count} mutations, against: {}",
        picked.len(),
        checks
            .iter()
            .map(|c| c.command.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let report = mutate::run(
        root,
        &checks,
        picked,
        count,
        Duration::from_secs(args.timeout.max(1)),
    )?;
    match format {
        Format::Json => print_json(&report),
        Format::Text => {
            if !report.baseline_passed {
                println!(
                    "The checks fail before any mutation ({}), so mutations can't be judged. Fix that first: ube run.",
                    report.baseline_failure.as_deref().unwrap_or("?")
                );
            } else {
                for m in report.mutants.iter().filter(|m| m.killed == Some(false)) {
                    println!("survived  {}:{}  {}", m.file, m.line, m.change);
                    println!("            {}", m.original.trim());
                    println!("         -> {}", m.mutated.trim());
                }
                println!(
                    "{} of {} mutations caught ({} candidates). {}",
                    report.killed,
                    report.killed + report.survived,
                    report.candidates,
                    if report.survived == 0 {
                        "Every one made a test fail."
                    } else {
                        "Each survivor is a line no test proves: add a test that would fail with it."
                    }
                );
            }
        }
    }
    Ok(if report.baseline_passed && report.survived == 0 {
        exit::OK
    } else {
        exit::CHECK_FAILED
    })
}
