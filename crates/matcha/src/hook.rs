//! `matcha hook claude-code`: a Claude Code `PostToolUse` hook on Bash that
//! records each command and an excerpt of its output in the branch's ledger
//! folder, so `ledger add verified --cmd` can attach it as evidence
//! (docs/decisions/0017-ledger.md).

use std::io::Read;
use std::path::PathBuf;

use mochi_index::RepoRoot;
use serde::Deserialize;

use crate::ledger::Store;

/// What Claude Code sends a `PostToolUse` hook (only the fields read here).
#[derive(Debug, Deserialize)]
struct HookInput {
    cwd: Option<PathBuf>,
    tool_name: Option<String>,
    tool_input: Option<ToolInput>,
    tool_response: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ToolInput {
    command: Option<String>,
}

/// Records the command, if it's a Bash call inside a git repo. Never fails
/// the agent's turn: anything it can't read or write is dropped, and it
/// prints nothing.
pub fn claude_code() {
    let mut stdin = String::new();
    if std::io::stdin().read_to_string(&mut stdin).is_err() {
        return;
    }
    let Ok(input) = serde_json::from_str::<HookInput>(&stdin) else {
        return;
    };
    if input.tool_name.as_deref() != Some("Bash") {
        return;
    }
    let Some(command) = input.tool_input.and_then(|t| t.command) else {
        return;
    };
    let cwd = input.cwd.unwrap_or_else(|| PathBuf::from("."));
    let Ok(root) = RepoRoot::discover(&cwd) else {
        return;
    };
    let Ok(store) = Store::open(&root) else {
        return;
    };
    let output = input.tool_response.map(output_text).unwrap_or_default();
    let _ = store.record_command(&command, &output);
}

/// A Bash tool response's text: `stdout` then `stderr` when it's an object,
/// or the string itself.
fn output_text(response: serde_json::Value) -> String {
    match response {
        serde_json::Value::String(s) => s,
        serde_json::Value::Object(map) => ["stdout", "stderr", "output"]
            .iter()
            .filter_map(|k| map.get(*k).and_then(|v| v.as_str()))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_bash_output_in_either_shape() {
        let v = serde_json::json!({"stdout": "out", "stderr": "err", "interrupted": false});
        assert_eq!(output_text(v), "out\nerr");
        assert_eq!(output_text(serde_json::json!("plain")), "plain");
        assert_eq!(output_text(serde_json::json!(3)), "");
    }
}
