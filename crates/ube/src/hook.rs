//! `ube hook claude-code`: a Claude Code `Stop` hook. When the agent ends
//! its turn with a change whose planned checks haven't passed on the exact
//! tree, it asks the agent to run them or say what's unproven. It stays out
//! of the way otherwise, and never blocks twice in a row
//! (docs/ube/agents.md).

use std::io::Read;
use std::path::PathBuf;

use mochi_index::RepoRoot;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct HookInput {
    cwd: Option<PathBuf>,
    /// True when the agent is already continuing because of a Stop hook.
    #[serde(default)]
    stop_hook_active: bool,
}

/// The reason to keep going, or `None` to let the agent stop.
pub fn claude_code(check: impl Fn(&RepoRoot) -> Option<String>) -> Option<String> {
    let mut stdin = String::new();
    std::io::stdin().read_to_string(&mut stdin).ok()?;
    let input: HookInput = serde_json::from_str(&stdin).ok()?;
    if input.stop_hook_active {
        return None;
    }
    let root = RepoRoot::discover(&input.cwd.unwrap_or_else(|| PathBuf::from("."))).ok()?;
    check(&root)
}
