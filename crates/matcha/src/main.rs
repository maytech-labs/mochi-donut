//! matcha: what a repo knows. Conventions (`census`), change recipes
//! (`recipe`), doc drift (`drift`), and a verified-facts ledger (`ledger`).
//! See docs/design.md.

mod census;
mod drift;
mod files;
mod hook;
mod ledger;
mod markdown;
mod mcp_tools;
mod recipe;
mod rules;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ArgGroup, Args, Parser, Subcommand};
use mochi_common::{Format, exit, print_json};
use mochi_index::{CoChange, RepoRoot};
use regex::Regex;

use recipe::Thresholds;

const AGENT_GUIDE: &str = include_str!("../AGENT_GUIDE.md");

#[derive(Debug, Parser)]
#[command(name = "matcha", version, about = "What a repo knows, with evidence.")]
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
    /// How does this repo usually do X? Counts the alternatives, with examples.
    Census(CensusArgs),
    /// What else usually changes when I change this?
    Recipe(RecipeArgs),
    /// Are the docs' claims about the code still true?
    Drift(DriftArgs),
    /// Record and check what was verified and what was assumed.
    Ledger(LedgerArgs),
    /// Serve matcha's commands as MCP tools on stdin and stdout.
    Mcp,
    /// Run as an agent hook: read the hook's JSON on stdin.
    Hook {
        #[arg(value_enum)]
        agent: HookAgent,
    },
    /// Print a short guide to matcha, written for AI agents.
    AgentGuide,
}

#[derive(Debug, Args)]
struct CensusArgs {
    /// A label for what you're asking about, echoed in the output.
    label: Option<String>,
    /// A regular expression for one way of doing it. Repeat for each alternative.
    #[arg(long = "alt", value_name = "REGEX")]
    alts: Vec<String>,
    /// Count every spelling of words starting with this stem instead.
    #[arg(long, value_name = "STEM", conflicts_with = "alts")]
    spelling: Option<String>,
    /// Only look under these paths. Repeatable.
    #[arg(long = "in", value_name = "PATH")]
    paths: Vec<String>,
    /// Examples to show per alternative.
    #[arg(long, default_value_t = 3)]
    examples: usize,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum HookAgent {
    /// A Claude Code `PostToolUse` hook on Bash that records commands and
    /// their output for `ledger add verified --cmd`.
    ClaudeCode,
}

#[derive(Debug, Args)]
struct LedgerArgs {
    #[command(subcommand)]
    command: LedgerCommand,
}

#[derive(Debug, Subcommand)]
enum LedgerCommand {
    /// Record a claim as verified (with evidence) or assumed.
    Add {
        #[arg(value_enum)]
        status: StatusArg,
        /// The claim, in one sentence.
        claim: String,
        #[command(flatten)]
        evidence: EvidenceArgs,
    },
    /// Mark an assumed claim verified, with evidence.
    Verify {
        /// The entry's number, from `matcha ledger list`.
        id: u32,
        #[command(flatten)]
        evidence: EvidenceArgs,
    },
    /// List this branch's claims.
    List,
    /// Flag assumed claims that added lines in docs or comments state.
    Check {
        /// Check the working tree's changes against --base (default HEAD).
        #[arg(long, required = true)]
        diff: bool,
        /// What to diff against: a commit, branch, or tag.
        #[arg(long, value_name = "REV", requires = "diff")]
        base: Option<String>,
    },
    /// Print the ledger as Markdown for a PR description.
    Summary,
    /// Move this branch's ledger to .matcha/ledger/archive/, after merging.
    Archive,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum StatusArg {
    Verified,
    Assumed,
}

#[derive(Debug, Args)]
struct EvidenceArgs {
    /// What shows it: a command's output, a URL, what you saw.
    #[arg(long, value_name = "TEXT")]
    evidence: Option<String>,
    /// A file, and optionally a line, that shows it: src/a.rs:12.
    #[arg(long, value_name = "PATH[:LINE]")]
    file: Option<String>,
    /// Attach the latest command the hook recorded that contains TEXT, with
    /// its output.
    #[arg(long, value_name = "TEXT")]
    cmd: Option<String>,
}

#[derive(Debug, Args)]
struct DriftArgs {
    /// Only check docs under these paths. Repeatable.
    #[arg(long = "in", value_name = "PATH")]
    paths: Vec<String>,
    /// Propose check comments for sentences that read like rules, with
    /// whether the code follows each today. Changes nothing.
    #[arg(long)]
    suggest: bool,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("mode").required(true).args(["check", "like", "replay"])))]
struct RecipeArgs {
    /// Check the working tree (or --commit) for missing companion changes.
    #[arg(long)]
    check: bool,
    /// Find past changes like this one and list the files they had in common.
    #[arg(long, value_name = "TEXT")]
    like: Option<String>,
    /// Measure how --check would have done on the last N commits.
    #[arg(long, value_name = "N")]
    replay: Option<usize>,
    /// With --check: check this commit against the history before it.
    #[arg(long, value_name = "REV", requires = "check")]
    commit: Option<String>,
    /// Commits of history to learn from.
    #[arg(long, default_value_t = 2000)]
    history: usize,
    /// Least number of earlier changes to a file before it predicts anything.
    #[arg(long, default_value_t = 6)]
    min_support: u32,
    /// Least share of those changes that included the companion, 0 to 1.
    #[arg(long, default_value_t = 0.9)]
    min_confidence: f64,
    /// Least lift: how much likelier than usual the companion must be to change.
    /// Filters out files that change in most commits anyway.
    #[arg(long, default_value_t = 2.0)]
    min_lift: f64,
    /// Ignore commits that changed more files than this.
    #[arg(long, default_value_t = 300)]
    max_files: usize,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = Format::from_json_flag(cli.json);

    if let Command::AgentGuide = cli.command {
        print!("{AGENT_GUIDE}");
        return ExitCode::from(exit::OK);
    }
    if let Command::Mcp = cli.command {
        // Each tool call finds its own repo, from `repo` or the working dir.
        return ExitCode::from(mcp_tools::serve());
    }
    if let Command::Hook {
        agent: HookAgent::ClaudeCode,
    } = cli.command
    {
        // The hook finds the repo from its input, and never blocks the agent.
        hook::claude_code();
        return ExitCode::from(exit::OK);
    }

    // Every repo command needs a repo; say so plainly before anything else.
    let start = cli.repo.unwrap_or_else(|| PathBuf::from("."));
    let root = match RepoRoot::discover(&start) {
        Ok(root) => root,
        Err(e) => {
            eprintln!("{e}. Run matcha inside a git repo, or pass --repo <path>.");
            return ExitCode::from(exit::USAGE);
        }
    };

    let result = match cli.command {
        Command::Census(args) => run_census(&root, args, format),
        Command::Recipe(args) => run_recipe(&root, args, format),
        Command::Drift(args) => run_drift(&root, args, format),
        Command::Ledger(args) => run_ledger(&root, args, format),
        Command::AgentGuide | Command::Hook { .. } | Command::Mcp => Ok(exit::OK),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(exit::USAGE)
        }
    }
}

fn run_census(root: &RepoRoot, args: CensusArgs, format: Format) -> Result<u8, String> {
    let files = files::text_files(root, &args.paths).map_err(|e| e.to_string())?;

    if let Some(stem) = args.spelling {
        let s = census::spelling(&stem, &files);
        match format {
            Format::Json => print_json(&s),
            Format::Text => {
                println!("Spellings of {stem}* ({} files searched)", s.files_searched);
                if s.variants.is_empty() {
                    println!("  none found");
                }
                for v in &s.variants {
                    println!(
                        "  {:<24} {:>6}   ({})   e.g. {}:{}",
                        v.word,
                        v.count,
                        breakdown(&v.by_language),
                        v.example.path,
                        v.example.line
                    );
                }
            }
        }
        return Ok(exit::OK);
    }

    if args.alts.is_empty() {
        return Err(
            "Give the alternatives to compare with --alt <regex> (repeat it), or count spellings with --spelling <stem>.\n  e.g. matcha census \"form state\" --alt 'useForm\\(' --alt 'useState<\\w*Draft>'".into(),
        );
    }
    let patterns = args
        .alts
        .iter()
        .map(|a| Regex::new(a).map_err(|e| format!("--alt {a:?} isn't a valid regex: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    let c = census::alternatives(args.label, &patterns, &files, args.examples);
    match format {
        Format::Json => print_json(&c),
        Format::Text => {
            let title = c.query.as_deref().unwrap_or("census");
            println!("{title} ({} files searched)", c.files_searched);
            for t in &c.alternatives {
                println!(
                    "  {:<32} {:>5} files {:>4.0}%  {:>6} matches   ({})",
                    t.pattern,
                    t.files,
                    t.share * 100.0,
                    t.matches,
                    breakdown(&t.by_language)
                );
                for e in &t.examples {
                    println!("      {}:{}  ({} matches)", e.path, e.line, e.matches);
                }
            }
        }
    }
    Ok(exit::OK)
}

fn run_recipe(root: &RepoRoot, args: RecipeArgs, format: Format) -> Result<u8, String> {
    let t = Thresholds {
        min_support: args.min_support,
        min_confidence: args.min_confidence,
        min_lift: args.min_lift,
        max_files: args.max_files,
    };
    let err = |e: mochi_index::IndexError| e.to_string();

    if let Some(n) = args.replay {
        let commits = root.history(None, args.history + n).map_err(err)?;
        let r = recipe::replay(&commits, n, t);
        match format {
            Format::Json => print_json(&r),
            Format::Text => {
                println!(
                    "Replayed {} commits, removing each file in turn ({} tests).",
                    r.commits_replayed, r.removals_tested
                );
                let precision = r.precision.map_or("n/a (no warnings)".to_owned(), |p| {
                    format!("{:.0}%", p * 100.0)
                });
                println!(
                    "  caught {} of {} removed files ({:.0}% recall)",
                    r.caught,
                    r.removals_tested,
                    r.recall * 100.0
                );
                println!(
                    "  {} false alarms on complete commits, in {} of {} commits ({} precision)",
                    r.false_alarms, r.noisy_commits, r.commits_replayed, precision
                );
                for c in r.catches.iter().take(10) {
                    println!(
                        "  {}  caught {} (changed with {} in {} of {})",
                        c.commit, c.missing, c.changed, c.together, c.of
                    );
                }
            }
        }
        return Ok(exit::OK);
    }

    if let Some(query) = args.like {
        let commits = root.history(None, args.history).map_err(err)?;
        let l = recipe::like(&query, &commits, 20);
        match format {
            Format::Json => print_json(&l),
            Format::Text => {
                if l.similar.is_empty() {
                    println!(
                        "No past commits look like {query:?}. Try other words from commit subjects."
                    );
                    return Ok(exit::OK);
                }
                println!("Based on {} similar changes:", l.similar.len());
                for c in l.similar.iter().take(5) {
                    println!("  {}  {}", c.hash, c.subject);
                }
                println!("Files they had in common:");
                for f in &l.files {
                    println!("  {:<60} {}/{}", f.path, f.commits, l.similar.len());
                }
            }
        }
        return Ok(exit::OK);
    }

    // --check
    let (changed, commits) = match &args.commit {
        Some(rev) => {
            let changed = root.commit_files(rev).map_err(err)?;
            let before = format!("{rev}^");
            (
                changed,
                root.history(Some(&before), args.history).map_err(err)?,
            )
        }
        None => (
            root.working_changes().map_err(err)?,
            root.history(None, args.history).map_err(err)?,
        ),
    };
    let cc = CoChange::build(&commits, args.max_files);
    let live = args.commit.is_none();
    let mut missing = recipe::check(&changed, &cc, t, |f| !live || root.path().join(f).exists());
    for m in &mut missing {
        m.fix = recipe::fix_hint(root.path(), &m.missing);
    }

    match format {
        Format::Json => print_json(&serde_json::json!({
            "changed": changed.len(),
            "commits_learned_from": cc.commits(),
            "missing": missing,
        })),
        Format::Text => {
            if missing.is_empty() {
                println!(
                    "Nothing missing: no companion file is usually changed with these {} files (learned from {} commits).",
                    changed.len(),
                    cc.commits()
                );
            }
            for m in &missing {
                println!("{} changed but {} didn't.", m.changed, m.missing);
                println!(
                    "  {} of {} earlier changes to {} changed both.",
                    m.together, m.of, m.changed
                );
                if let Some(fix) = &m.fix {
                    println!("  Usual fix: {fix}");
                }
            }
        }
    }
    Ok(if missing.is_empty() {
        exit::OK
    } else {
        exit::CHECK_FAILED
    })
}

fn run_drift(root: &RepoRoot, args: DriftArgs, format: Format) -> Result<u8, String> {
    let err = |e: mochi_index::IndexError| e.to_string();
    let tracked = root.tracked_files().map_err(err)?;
    let all = files::text_files(root, &[]).map_err(err)?;
    let licensed = drift::licensed_dirs(&tracked);
    let docs: Vec<&files::SourceFile> = all
        .iter()
        .filter(|f| drift::is_own_doc(&f.path, &licensed))
        .filter(|f| args.paths.is_empty() || args.paths.iter().any(|p| files::under(&f.path, p)))
        .collect();
    let repo = drift::Repo::new(root, tracked, &all, &docs);

    if args.suggest {
        let suggestions = repo.suggest(&docs);
        match format {
            Format::Json => print_json(&serde_json::json!({ "suggestions": suggestions })),
            Format::Text => {
                for s in &suggestions {
                    println!("{}:{}  {}", s.doc, s.line, s.sentence);
                    let now = if s.holds_now {
                        "holds now"
                    } else {
                        "fails now"
                    };
                    println!("  {}", s.comment);
                    println!("  {now}: {}", s.evidence);
                }
                println!(
                    "{} suggestions. Review each, then paste the comment on the line before its sentence.",
                    suggestions.len()
                );
            }
        }
        return Ok(exit::OK);
    }

    let r = repo.check(&docs);

    match format {
        Format::Json => print_json(&r),
        Format::Text => {
            if r.docs_checked == 0 {
                println!("No Markdown docs to check (looked for tracked .md and .mdx files).");
                return Ok(exit::OK);
            }
            for f in &r.failing {
                let claim = match f.kind {
                    drift::Kind::Link | drift::Kind::Anchor => format!("({})", f.claim),
                    drift::Kind::Rule => format!("\"{}\"", f.claim),
                    _ => format!("`{}`", f.claim),
                };
                println!("{}:{}", f.doc, f.line);
                println!("  {claim}");
                println!("  FAILS: {}", f.reason);
                println!("  {}", f.fix);
            }
            println!(
                "Checked {} claims in {} docs against {} source files: {} fail, {} hold. Skipped {} code spans and links that make no checkable claim.",
                r.claims_checked,
                r.docs_checked,
                r.source_files_searched,
                r.failing.len(),
                r.holds,
                r.skipped
            );
        }
    }
    Ok(if r.failing.is_empty() {
        exit::OK
    } else {
        exit::CHECK_FAILED
    })
}

fn run_ledger(root: &RepoRoot, args: LedgerArgs, format: Format) -> Result<u8, String> {
    let store = ledger::Store::open(root)?;
    match args.command {
        LedgerCommand::Add {
            status,
            claim,
            evidence,
        } => {
            if claim.trim().is_empty() {
                return Err(
                    "Give the claim in one sentence: matcha ledger add assumed \"<claim>\"".into(),
                );
            }
            let status = match status {
                StatusArg::Verified => ledger::Status::Verified,
                StatusArg::Assumed => ledger::Status::Assumed,
            };
            let evidence = gather_evidence(root, &store, &evidence)?;
            if status == ledger::Status::Verified && evidence.is_none() {
                return Err("A verified claim needs evidence: --evidence \"<what you saw>\", --file <path:line>, or --cmd <text from a command you ran>. Without it, record it as assumed.".into());
            }
            let entry = store.add(status, &claim, evidence)?;
            print_entry(&entry, "Recorded", format);
            Ok(exit::OK)
        }
        LedgerCommand::Verify { id, evidence } => {
            let Some(evidence) = gather_evidence(root, &store, &evidence)? else {
                return Err("Say what verified it: --evidence \"<what you saw>\", --file <path:line>, or --cmd <text from a command you ran>.".into());
            };
            let entry = store.verify(id, evidence)?;
            print_entry(&entry, "Verified", format);
            Ok(exit::OK)
        }
        LedgerCommand::List => {
            let entries = store.entries()?;
            match format {
                Format::Json => print_json(&serde_json::json!({
                    "branch": store.branch,
                    "entries": entries,
                })),
                Format::Text => {
                    if entries.is_empty() {
                        println!(
                            "No claims recorded on {}. Add one with: matcha ledger add <verified|assumed> \"<claim>\"",
                            store.branch
                        );
                    }
                    for e in &entries {
                        let status = match e.status {
                            ledger::Status::Verified => "verified",
                            ledger::Status::Assumed => "assumed ",
                        };
                        println!("{:>3}  {status}  {}", e.id, e.claim);
                        if let Some(ev) = &e.evidence {
                            println!("               {}", ev.describe());
                        }
                    }
                }
            }
            Ok(exit::OK)
        }
        LedgerCommand::Check { diff: _, base } => {
            let entries = store.entries()?;
            let added = root
                .added_lines(base.as_deref())
                .map_err(|e| e.to_string())?;
            let flags = ledger::check(&entries, &added);
            let assumed = entries
                .iter()
                .filter(|e| e.status == ledger::Status::Assumed)
                .count();
            match format {
                Format::Json => print_json(&serde_json::json!({
                    "branch": store.branch,
                    "assumed": assumed,
                    "lines_checked": added.len(),
                    "flagged": flags,
                })),
                Format::Text => {
                    for f in &flags {
                        println!("{}:{} states an assumed claim:", f.path, f.line);
                        println!("  \"{}\"", f.text);
                        println!(
                            "  matches #{} \"{}\" (assumed, no evidence; {} of {} words)",
                            f.id, f.claim, f.shared, f.of
                        );
                    }
                    if flags.is_empty() {
                        println!(
                            "No assumed claims in the {} added lines ({assumed} assumed claims on {}).",
                            added.len(),
                            store.branch
                        );
                    } else {
                        println!(
                            "Verify each and run `matcha ledger verify <id> --evidence ...`, or mark it in the text as unverified."
                        );
                    }
                }
            }
            Ok(if flags.is_empty() {
                exit::OK
            } else {
                exit::CHECK_FAILED
            })
        }
        LedgerCommand::Summary => {
            let entries = store.entries()?;
            let markdown = ledger::summary(&entries);
            match format {
                Format::Json => print_json(&serde_json::json!({
                    "branch": store.branch,
                    "markdown": markdown,
                })),
                Format::Text => {
                    if markdown.is_empty() {
                        println!("No claims recorded on {}.", store.branch);
                    } else {
                        println!("{markdown}");
                    }
                }
            }
            Ok(exit::OK)
        }
        LedgerCommand::Archive => {
            let to = store.archive()?;
            match format {
                Format::Json => print_json(&serde_json::json!({
                    "branch": store.branch,
                    "archived_to": to,
                })),
                Format::Text => match &to {
                    Some(p) => println!("Archived {}'s ledger to {}.", store.branch, p.display()),
                    None => println!("No ledger for {} to archive.", store.branch),
                },
            }
            Ok(exit::OK)
        }
    }
}

fn gather_evidence(
    root: &RepoRoot,
    store: &ledger::Store,
    args: &EvidenceArgs,
) -> Result<Option<ledger::Evidence>, String> {
    let mut ev = match &args.file {
        Some(spec) => ledger::file_evidence(root.path(), spec)?,
        None => ledger::Evidence::default(),
    };
    if let Some(needle) = &args.cmd {
        let Some(record) = store.find_command(needle)? else {
            return Err(format!(
                "No recorded command contains {needle:?}. Commands are recorded by the hook (`matcha hook claude-code`, see `matcha agent-guide`); without it, paste what you saw with --evidence."
            ));
        };
        ev.command = Some(record.command);
        ev.output = Some(record.output);
    }
    ev.text = args.evidence.clone();
    Ok((ev != ledger::Evidence::default()).then_some(ev))
}

fn print_entry(entry: &ledger::Entry, verb: &str, format: Format) {
    match format {
        Format::Json => print_json(&serde_json::json!({ "entry": entry })),
        Format::Text => {
            let status = match entry.status {
                ledger::Status::Verified => "verified",
                ledger::Status::Assumed => "assumed",
            };
            println!("{verb} #{} as {status}: {}", entry.id, entry.claim);
            if let Some(ev) = &entry.evidence {
                println!("  evidence: {}", ev.describe());
            }
        }
    }
}

/// `Rust 66, TypeScript 20` from a count per language, largest first.
fn breakdown(by_language: &std::collections::BTreeMap<&'static str, usize>) -> String {
    let mut v: Vec<(&&str, &usize)> = by_language.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    v.iter()
        .map(|(l, n)| format!("{l} {n}"))
        .collect::<Vec<_>>()
        .join(", ")
}
