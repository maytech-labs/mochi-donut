//! taro: the forge and the team. Whether a PR can merge (`ready`), open
//! review threads (`reviews`), who owns files (`owners`), flaky checks
//! (`flaky`), and branches proved merged (`cleanup`). See docs/taro/.

mod cleanup;
mod flaky;
mod forge;
mod mcp_tools;
mod owners;
mod ready;
mod reviews;

use std::collections::BTreeMap;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use mochi_common::{Format, exit, print_json};

use crate::forge::{Repo, git};

const AGENT_GUIDE: &str = include_str!("../AGENT_GUIDE.md");

#[derive(Debug, Parser)]
#[command(
    name = "taro",
    version,
    about = "The forge and the team: what a PR needs to merge, and what's safe to clean up."
)]
struct Cli {
    /// Print one JSON object on stdout instead of text.
    #[arg(long, global = true)]
    json: bool,

    /// The repo on the forge: host/owner/repo, or owner/repo for github.com.
    /// Defaults to the push remote's.
    #[arg(long, global = true, value_name = "HOST/OWNER/REPO")]
    repo: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Can this PR merge? If not, every blocker and what to do.
    Ready {
        /// The PR number. Default: the current branch's.
        pr: Option<u64>,
    },
    /// Open review threads, as a checklist.
    Reviews {
        /// The PR number. Default: the current branch's.
        pr: Option<u64>,
        /// Include resolved threads.
        #[arg(long)]
        all: bool,
    },
    /// Who owns these files, and who has changed them most.
    Owners(OwnersArgs),
    /// Which checks fail and then pass on the same commit.
    Flaky {
        /// How far back: 30d, 14d.
        #[arg(long, default_value = "30d", value_name = "AGE")]
        since: String,
    },
    /// Which branches are merged, proved, and which aren't.
    Cleanup(CleanupArgs),
    /// Serve ready, reviews, owners, and flaky as MCP tools on stdin and stdout.
    Mcp,
    /// Print a short guide to taro, written for AI agents.
    AgentGuide,
}

#[derive(Debug, Args)]
struct OwnersArgs {
    /// Files or directories.
    paths: Vec<String>,
    /// The files this branch changes against --base, plus uncommitted ones.
    #[arg(long, conflicts_with = "paths")]
    diff: bool,
    /// The branch the change is against. Default: the remote's default branch.
    #[arg(long, value_name = "REV")]
    base: Option<String>,
    /// How much history to count authors in: 1.year, 6.months.
    #[arg(long, default_value = "1.year", value_name = "AGE")]
    since: String,
}

#[derive(Debug, Args)]
struct CleanupArgs {
    /// The branch merges land in. Default: the remote's default branch.
    #[arg(long, value_name = "REV")]
    base: Option<String>,
    /// Include the push remote's branches, not only local ones.
    #[arg(long)]
    remote: bool,
    /// Delete the branches proved merged.
    #[arg(long)]
    delete: bool,
    /// With --delete: don't ask first. Required when there's no terminal to ask on.
    #[arg(long, requires = "delete")]
    yes: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = Format::from_json_flag(cli.json);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let result = match cli.command {
        Command::AgentGuide => {
            print!("{AGENT_GUIDE}");
            Ok(exit::OK)
        }
        Command::Mcp => Ok(mcp_tools::serve()),
        Command::Ready { pr } => {
            Repo::find(cli.repo.as_deref(), &cwd).and_then(|r| cmd_ready(&r, pr, format))
        }
        Command::Reviews { pr, all } => {
            Repo::find(cli.repo.as_deref(), &cwd).and_then(|r| cmd_reviews(&r, pr, all, format))
        }
        Command::Flaky { since } => {
            Repo::find(cli.repo.as_deref(), &cwd).and_then(|r| cmd_flaky(&r, &since, format))
        }
        Command::Owners(args) => local_root(&cwd).and_then(|root| cmd_owners(&root, &args, format)),
        Command::Cleanup(args) => {
            local_root(&cwd).and_then(|root| cmd_cleanup(&root, &args, format))
        }
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(exit::USAGE)
        }
    }
}

fn local_root(cwd: &Path) -> Result<PathBuf, String> {
    git(cwd, &["rev-parse", "--show-toplevel"])
        .map(|r| PathBuf::from(r.trim()))
        .map_err(|_| "Not in a git repo: run taro inside a clone.".to_owned())
}

/// The remote's default branch as a remote-tracking ref (`upstream/main`),
/// else a local `main` or `master`.
fn default_base(root: &Path) -> String {
    let remotes = git(root, &["remote"]).unwrap_or_default();
    for r in remotes.lines() {
        if let Ok(head) = git(
            root,
            &[
                "symbolic-ref",
                "--short",
                "-q",
                &format!("refs/remotes/{r}/HEAD"),
            ],
        ) {
            return head.trim().to_owned();
        }
    }
    for b in ["main", "master"] {
        if git(root, &["rev-parse", "--verify", "-q", b]).is_ok() {
            return b.to_owned();
        }
    }
    "main".to_owned()
}

fn print_items(items: &[ready::Item]) {
    for i in items {
        let mark = match i.state {
            ready::State::Ok => "ok   ",
            ready::State::Warn => "warn ",
            ready::State::Block => "BLOCK",
            ready::State::Unknown => "?    ",
        };
        println!("{mark}  {:<11} {}", i.name, i.detail);
        if let Some(fix) = &i.fix {
            println!("       {:<11} {fix}", "");
        }
    }
}

fn cmd_ready(repo: &Repo, pr: Option<u64>, format: Format) -> Result<u8, String> {
    let n = forge::pr_number(repo, pr)?;
    let fields = "number,title,url,state,isDraft,mergeable,mergeStateStatus,reviewDecision,headRefName,baseRefName,statusCheckRollup,files";
    let view = forge::gh(&[
        "pr",
        "view",
        &n.to_string(),
        "-R",
        &repo.full(),
        "--json",
        fields,
    ])?;
    let base = view["baseRefName"].as_str().unwrap_or("main").to_owned();
    let answer = forge::graphql(repo, ready::QUERY, n).ok();
    let reported = answer.as_ref().and_then(ready::required_reported);
    let unresolved = answer.as_ref().and_then(ready::unresolved);
    let rules = forge::api_optional(
        repo,
        &format!("repos/{}/{}/rules/branches/{base}", repo.owner, repo.name),
    )?;
    let protection = protection_checks(repo, &base)?;
    let required = required_checks(reported, rules.as_ref(), protection.as_ref());
    let merged_json = forge::gh(&[
        "pr",
        "list",
        "-R",
        &repo.full(),
        "--state",
        "merged",
        "-L",
        "30",
        "--json",
        "title,files",
    ])?;
    let merged: Vec<ready::MergedPr> = merged_json
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| ready::MergedPr {
            title: p["title"].as_str().unwrap_or("").to_owned(),
            files: p["files"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|f| f["path"].as_str().map(str::to_owned))
                .collect(),
        })
        .collect();
    let items = ready::ready(&ready::Facts {
        pr: &view,
        required,
        unresolved,
        merged: &merged,
    });
    let blocked = items.iter().any(|i| i.state == ready::State::Block);
    match format {
        Format::Json => print_json(
            &serde_json::json!({ "pr": n, "url": view["url"], "ready": !blocked, "items": items }),
        ),
        Format::Text => {
            println!("PR #{n}: {}", view["title"].as_str().unwrap_or(""));
            print_items(&items);
            println!(
                "{}",
                if blocked {
                    "Not ready to merge."
                } else {
                    "Ready to merge, as far as the forge says."
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

/// The base branch's classic protection checks: `Some` with the answer,
/// `Some(null)` when the branch isn't protected or requires no checks, and
/// `None` when it can't be read (it needs admin rights, or a paid plan for
/// a private repo).
fn protection_checks(repo: &Repo, base: &str) -> Result<Option<serde_json::Value>, String> {
    let path = format!(
        "repos/{}/{}/branches/{base}/protection/required_status_checks",
        repo.owner, repo.name
    );
    match forge::api(repo, &path) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.contains("not protected") || e.contains("not enabled") => {
            Ok(Some(serde_json::Value::Null))
        }
        Err(e) if e.contains("403") || e.contains("404") || e.contains("Not Found") => Ok(None),
        Err(e) => Err(e),
    }
}

/// The required check names, or `None` when they can't be told.
///
/// `reported` are the reported checks GitHub marks required for this PR.
/// Rulesets and branch protection add required checks that haven't
/// reported. Without `reported`, rulesets alone can't say: classic
/// protection may require more, so it takes a readable protection answer.
fn required_checks(
    reported: Option<Vec<String>>,
    rules: Option<&serde_json::Value>,
    protection: Option<&serde_json::Value>,
) -> Option<Vec<String>> {
    let mut declared: Vec<String> = Vec::new();
    for r in rules.and_then(|r| r.as_array()).into_iter().flatten() {
        if r["type"] == "required_status_checks" {
            for c in r["parameters"]["required_status_checks"]
                .as_array()
                .into_iter()
                .flatten()
            {
                declared.extend(c["context"].as_str().map(str::to_owned));
            }
        }
    }
    if let Some(p) = protection {
        declared.extend(
            p["contexts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| c.as_str().map(str::to_owned)),
        );
        declared.extend(
            p["checks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| c["context"].as_str().map(str::to_owned)),
        );
    }
    let mut names = match reported {
        Some(r) => r,
        None if protection.is_some() => Vec::new(),
        None => return None,
    };
    names.extend(declared);
    names.sort();
    names.dedup();
    Some(names)
}

fn cmd_reviews(repo: &Repo, pr: Option<u64>, all: bool, format: Format) -> Result<u8, String> {
    let n = forge::pr_number(repo, pr)?;
    let threads: Vec<reviews::Thread> = reviews::threads(&forge::graphql(repo, reviews::QUERY, n)?)
        .into_iter()
        .filter(|t| all || !t.resolved)
        .collect();
    let open = threads.iter().filter(|t| !t.resolved).count();
    match format {
        Format::Json => {
            print_json(&serde_json::json!({ "pr": n, "unresolved": open, "threads": threads }))
        }
        Format::Text => {
            for t in &threads {
                let at = t.line.map_or(t.path.clone(), |l| format!("{}:{l}", t.path));
                let flags = [(t.resolved, "resolved"), (t.outdated, "code changed since")]
                    .iter()
                    .filter(|(on, _)| *on)
                    .map(|(_, s)| *s)
                    .collect::<Vec<_>>()
                    .join(", ");
                println!(
                    "{}  {at}  from {}{}",
                    if t.resolved { "[x]" } else { "[ ]" },
                    t.reviewer,
                    if flags.is_empty() {
                        String::new()
                    } else {
                        format!("  ({flags})")
                    }
                );
                let body: String = t.last_comment.chars().take(240).collect();
                for l in body.lines().take(4) {
                    println!("      {}: {l}", t.last_author);
                }
                println!("      {}", t.url);
            }
            println!(
                "{open} unresolved review {} on PR #{n}.",
                if open == 1 { "thread" } else { "threads" }
            );
        }
    }
    Ok(if open == 0 {
        exit::OK
    } else {
        exit::CHECK_FAILED
    })
}

fn cmd_owners(root: &Path, args: &OwnersArgs, format: Format) -> Result<u8, String> {
    let paths: Vec<String> = if args.diff || args.paths.is_empty() {
        let base = args.base.clone().unwrap_or_else(|| default_base(root));
        let range = format!("{base}...HEAD");
        let mut files: Vec<String> = git(root, &["diff", "--name-only", &range])?
            .lines()
            .map(str::to_owned)
            .collect();
        files.extend(
            git(root, &["diff", "--name-only", "HEAD"])?
                .lines()
                .map(str::to_owned),
        );
        files.retain(|f| !f.is_empty());
        files.sort();
        files.dedup();
        if files.is_empty() {
            return Err(format!(
                "No changes against {base}. Name the paths: taro owners <path>..."
            ));
        }
        files
    } else {
        args.paths.clone()
    };
    let o = owners::owners(root, &paths, &args.since)?;
    match format {
        Format::Json => print_json(&o),
        Format::Text => {
            match &o.codeowners_file {
                Some(f) => {
                    let mut seen: BTreeMap<String, Vec<&str>> = BTreeMap::new();
                    for p in &o.paths {
                        let key = if p.owners.is_empty() {
                            "(no owner)".to_owned()
                        } else {
                            p.owners.join(" ")
                        };
                        seen.entry(key).or_default().push(&p.path);
                    }
                    println!("From {f}:");
                    for (owners, files) in &seen {
                        let more = if files.len() > 3 {
                            format!(" and {} more", files.len() - 3)
                        } else {
                            String::new()
                        };
                        println!(
                            "  {owners:<30} {}{more}",
                            files.iter().take(3).copied().collect::<Vec<_>>().join(", ")
                        );
                    }
                }
                None => println!("No CODEOWNERS file (.github/, the root, or docs/)."),
            }
            if !o.frequent.is_empty() {
                println!("Changed these most since {}:", args.since);
                for (a, n) in &o.frequent {
                    println!(
                        "  {a:<40} {n} {}",
                        if *n == 1 { "commit" } else { "commits" }
                    );
                }
            }
            if o.suggested.is_empty() {
                println!("No one to suggest: no owners and no other recent authors.");
            } else {
                println!("Suggested reviewers: {}", o.suggested.join(", "));
            }
        }
    }
    Ok(exit::OK)
}

fn cmd_flaky(repo: &Repo, since: &str, format: Format) -> Result<u8, String> {
    let days: u64 = since
        .trim_end_matches('d')
        .parse()
        .map_err(|_| format!("{since:?} isn't an age in days: write 30d or 14d"))?;
    let from =
        mochi_common::time::rfc3339(mochi_common::time::unix_now().saturating_sub(days * 86_400));
    let date = &from[..10];
    let mut runs = Vec::new();
    for page in 1..=5 {
        let path = format!(
            "repos/{}/{}/actions/runs?per_page=100&page={page}&created=%3E%3D{date}",
            repo.owner, repo.name
        );
        let batch = flaky::runs(&forge::api(repo, &path)?);
        let done = batch.len() < 100;
        runs.extend(batch);
        if done {
            break;
        }
    }
    let mut jobs: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    for r in runs.iter().filter(|r| {
        r.attempt > 1 && matches!(r.conclusion.as_str(), "success" | "neutral" | "skipped")
    }) {
        let last = forge::api(
            repo,
            &format!(
                "repos/{}/{}/actions/runs/{}/jobs",
                repo.owner, repo.name, r.id
            ),
        )?;
        let passed: Vec<&str> = last["jobs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|j| j["conclusion"] == "success")
            .filter_map(|j| j["name"].as_str())
            .collect();
        let mut failed_before: Vec<String> = Vec::new();
        for a in 1..r.attempt {
            let earlier = forge::api(
                repo,
                &format!(
                    "repos/{}/{}/actions/runs/{}/attempts/{a}/jobs",
                    repo.owner, repo.name, r.id
                ),
            )?;
            for j in earlier["jobs"].as_array().into_iter().flatten() {
                if let Some(name) = j["name"].as_str()
                    && matches!(j["conclusion"].as_str(), Some("failure" | "timed_out"))
                    && passed.contains(&name)
                    && !failed_before.iter().any(|f| f == name)
                {
                    failed_before.push(name.to_owned());
                }
            }
        }
        jobs.insert(r.id, failed_before);
    }
    let found = flaky::flaky(&runs, &jobs);
    match format {
        Format::Json => {
            print_json(&serde_json::json!({ "since": date, "runs": runs.len(), "flaky": found }))
        }
        Format::Text => {
            if found.is_empty() {
                println!(
                    "No check failed and then passed on the same commit since {date} ({} runs).",
                    runs.len()
                );
            }
            for f in &found {
                println!(
                    "  {:<40} {} of {} runs ({:.0}%), last {}  {}",
                    f.check,
                    f.flakes,
                    f.runs,
                    f.rate * 100.0,
                    &f.last[..10.min(f.last.len())],
                    f.example
                );
            }
        }
    }
    Ok(exit::OK)
}

fn cmd_cleanup(root: &Path, args: &CleanupArgs, format: Format) -> Result<u8, String> {
    let base = args.base.clone().unwrap_or_else(|| default_base(root));
    let remote = if args.remote {
        let r = base
            .split_once('/')
            .map(|(r, _)| r.to_owned())
            .filter(|r| git(root, &["remote", "get-url", r]).is_ok());
        Some(r.ok_or_else(|| format!("--remote needs a remote-tracking base, like upstream/main; {base} isn't one. Pass --base <remote>/<branch>."))?)
    } else {
        None
    };
    let branches = cleanup::branches(root, &base, remote.as_deref())?;
    let (safe, kept): (Vec<&cleanup::Branch>, Vec<&cleanup::Branch>) =
        branches.iter().partition(|b| b.proof.is_some());

    if !args.delete {
        match format {
            Format::Json => print_json(&serde_json::json!({ "base": base, "branches": branches })),
            Format::Text => {
                for b in &safe {
                    println!("merged    {:<40} ({})", b.name, proof_label(b.proof));
                }
                for b in &kept {
                    println!(
                        "keep      {:<40} {}",
                        b.name,
                        b.unsafe_because.as_deref().unwrap_or("")
                    );
                }
                println!(
                    "{} of {} branches are proved merged into {base}. `taro cleanup{} --delete` deletes them.",
                    safe.len(),
                    branches.len(),
                    if args.remote { " --remote" } else { "" }
                );
            }
        }
        return Ok(exit::OK);
    }

    if safe.is_empty() {
        println!("No branch is proved merged into {base}; nothing to delete.");
        return Ok(exit::OK);
    }
    if !args.yes {
        if !std::io::stdin().is_terminal() {
            return Err(
                "Deleting branches needs a yes: run it in a terminal, or pass --yes.".into(),
            );
        }
        println!("Delete {} branches proved merged into {base}?", safe.len());
        for b in &safe {
            println!("  {}", b.name);
        }
        print!("[y/N] ");
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        let _ = std::io::stdin().lock().read_line(&mut answer);
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            println!("Nothing deleted.");
            return Ok(exit::OK);
        }
    }
    let mut deleted = Vec::new();
    let mut failed = Vec::new();
    for b in &safe {
        let result = match &b.remote {
            Some(r) => {
                let short = b.name.strip_prefix(&format!("{r}/")).unwrap_or(&b.name);
                git(root, &["push", r, "--delete", short])
            }
            None => git(root, &["branch", "-D", &b.name]),
        };
        match result {
            Ok(_) => deleted.push(b.name.clone()),
            Err(e) => failed.push((b.name.clone(), e)),
        }
    }
    match format {
        Format::Json => {
            print_json(&serde_json::json!({ "base": base, "deleted": deleted, "failed": failed }))
        }
        Format::Text => {
            for d in &deleted {
                println!("deleted   {d}");
            }
            for (d, e) in &failed {
                println!("FAILED    {d}: {e}");
            }
        }
    }
    Ok(if failed.is_empty() {
        exit::OK
    } else {
        exit::CHECK_FAILED
    })
}

fn proof_label(p: Option<cleanup::Proof>) -> &'static str {
    match p {
        Some(cleanup::Proof::Ancestor) => "in its history",
        Some(cleanup::Proof::EveryPatch) => "every commit's change is there",
        Some(cleanup::Proof::Squashed) => "squashed into one commit there",
        Some(cleanup::Proof::SameContent) => "its files match",
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn required_checks_are_unknown_unless_github_says() {
        let rules = json!([{"type": "required_status_checks", "parameters": {"required_status_checks": [{"context": "deploy"}]}}]);
        let empty = json!([]);
        // isRequired answered: those, plus rules' checks not yet reported.
        assert_eq!(
            required_checks(Some(vec!["linux".into()]), Some(&rules), None),
            Some(vec!["deploy".to_owned(), "linux".to_owned()])
        );
        // No isRequired and protection unreadable: an empty ruleset list
        // doesn't mean nothing is required.
        assert_eq!(required_checks(None, Some(&empty), None), None);
        // An unprotected branch is an answer.
        assert_eq!(
            required_checks(None, Some(&empty), Some(&serde_json::Value::Null)),
            Some(vec![])
        );
        let prot = json!({"contexts": ["ci/legacy"], "checks": [{"context": "linux"}]});
        assert_eq!(
            required_checks(None, None, Some(&prot)),
            Some(vec!["ci/legacy".to_owned(), "linux".to_owned()])
        );
    }
}
