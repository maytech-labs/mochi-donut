//! matcha: what a repo knows. Conventions (`census`), change recipes
//! (`recipe`), doc drift (`drift`), and a verified-facts ledger (`ledger`).
//! See docs/design.md.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use mochi_common::{Format, exit, not_implemented};
use mochi_index::RepoRoot;

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
    Census {
        /// A named pattern, or a description of what to look for.
        query: Option<String>,
    },
    /// What else usually changes when I change this?
    Recipe {
        /// Find past changes like this one ("add an endpoint").
        #[arg(long)]
        like: Option<String>,
        /// Check the working tree for missing companion changes.
        #[arg(long)]
        check: bool,
    },
    /// Are the docs' claims about the code still true?
    Drift,
    /// Record and check what was verified and what was assumed.
    Ledger,
    /// Print a short guide to matcha, written for AI agents.
    AgentGuide,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = Format::from_json_flag(cli.json);

    let name = match &cli.command {
        Command::AgentGuide => {
            print!("{AGENT_GUIDE}");
            return ExitCode::from(exit::OK);
        }
        Command::Census { .. } => "census",
        Command::Recipe { .. } => "recipe",
        Command::Drift => "drift",
        Command::Ledger => "ledger",
    };

    // Every repo command needs a repo; say so plainly before anything else.
    let start = cli.repo.unwrap_or_else(|| PathBuf::from("."));
    if let Err(e) = RepoRoot::discover(&start) {
        eprintln!("{e}. Run matcha inside a git repo, or pass --repo <path>.");
        return ExitCode::from(exit::USAGE);
    }

    ExitCode::from(not_implemented(format, "matcha", name))
}
