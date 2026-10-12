//! yuzu: what your environment is. Shows and classifies the targets a command
//! would touch (`status`, `check`) and checks services from where the user is
//! (`probe`). See docs/design.md.

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use mochi_common::{Format, exit, not_implemented};

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
        /// The command to check, after `--`.
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Check a URL the way the user's browser would, and report the exact failure.
    Probe { url: String },
    /// Print a short guide to yuzu, written for AI agents.
    AgentGuide,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = Format::from_json_flag(cli.json);

    let name = match cli.command.unwrap_or(Command::Status) {
        Command::AgentGuide => {
            print!("{AGENT_GUIDE}");
            return ExitCode::from(exit::OK);
        }
        Command::Status => "status",
        Command::Check { .. } => "check",
        Command::Probe { .. } => "probe",
    };
    ExitCode::from(not_implemented(format, "yuzu", name))
}
