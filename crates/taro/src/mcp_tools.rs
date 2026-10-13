//! taro's MCP tools: each maps its arguments to a taro command line
//! (docs/taro/agents.md).

use mochi_common::mcp::{Args, Server, Tool};
use serde_json::{Value, json};

pub const INSTRUCTIONS: &str = "taro knows the forge and the team: whether a PR can merge and what's blocking it, open review threads, who owns files, which checks are flaky, and which branches are proved merged. Use `ready` before asking to merge, and `flaky` before debugging a CI failure. Every call goes to GitHub through gh.";

fn base(a: &Args, cmd: &str) -> Result<Vec<String>, String> {
    let mut argv = vec![cmd.to_owned()];
    a.push_opt(&mut argv, "repo", "--repo")?;
    Ok(argv)
}

fn with_pr(v: &Value, cmd: &str) -> Result<Vec<String>, String> {
    let a = Args(v);
    let mut argv = base(&a, cmd)?;
    argv.extend(a.str("pr")?);
    Ok(argv)
}

fn ready(v: &Value) -> Result<Vec<String>, String> {
    with_pr(v, "ready")
}

fn reviews(v: &Value) -> Result<Vec<String>, String> {
    with_pr(v, "reviews")
}

fn owners(v: &Value) -> Result<Vec<String>, String> {
    let a = Args(v);
    let mut argv = base(&a, "owners")?;
    let paths = a.list("paths")?;
    if paths.is_empty() {
        argv.push("--diff".into());
    } else {
        argv.extend(paths);
    }
    Ok(argv)
}

fn flaky(v: &Value) -> Result<Vec<String>, String> {
    let a = Args(v);
    let mut argv = base(&a, "flaky")?;
    a.push_opt(&mut argv, "since", "--since")?;
    Ok(argv)
}

fn pr_props() -> Value {
    json!({
        "pr": { "type": "integer", "description": "The PR number. Default: the current branch's." },
        "repo": { "type": "string", "description": "host/owner/repo. Default: the push remote's." },
    })
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "ready",
            description: "Use before asking to merge a PR, or when asked whether it can merge: every blocker (failing or pending checks, requested changes, unresolved threads, conflicts, draft) and the repo's own conventions from its merged PRs, each with what to do.",
            input_schema: json!({ "type": "object", "properties": pr_props() }),
            args: ready,
        },
        Tool {
            name: "reviews",
            description: "Use when addressing review feedback: unresolved review threads as a checklist, with file, line, reviewer, the latest comment, and whether the code changed since.",
            input_schema: json!({ "type": "object", "properties": pr_props() }),
            args: reviews,
        },
        Tool {
            name: "owners",
            description: "Use when choosing reviewers or asking who knows some code: CODEOWNERS for the paths (default: the change against the base), and who changed them most this year.",
            input_schema: json!({ "type": "object", "properties": {
                "paths": { "type": "array", "items": { "type": "string" }, "description": "Files or directories. Default: the change." },
                "repo": { "type": "string", "description": "host/owner/repo. Default: the push remote's." },
            } }),
            args: owners,
        },
        Tool {
            name: "flaky",
            description: "Use before debugging a CI failure: which checks failed and then passed on the same commit recently, with their rates, so a known flake isn't mistaken for your bug.",
            input_schema: json!({ "type": "object", "properties": {
                "since": { "type": "string", "description": "How far back: 30d (default), 14d." },
                "repo": { "type": "string", "description": "host/owner/repo. Default: the push remote's." },
            } }),
            args: flaky,
        },
    ]
}

pub fn serve() -> u8 {
    let tools = tools();
    mochi_common::mcp::serve(&Server {
        name: "taro",
        version: env!("CARGO_PKG_VERSION"),
        instructions: INSTRUCTIONS,
        tools: &tools,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_arguments_to_command_lines() {
        let tools = tools();
        let argv = |name: &str, v: Value| {
            (tools.iter().find(|t| t.name == name).unwrap().args)(&v).unwrap()
        };
        assert_eq!(argv("ready", json!({"pr": 6})), ["ready", "6"]);
        assert_eq!(argv("owners", json!({})), ["owners", "--diff"]);
        assert_eq!(
            argv("owners", json!({"paths": ["crates/yuzu"]})),
            ["owners", "crates/yuzu"]
        );
        assert_eq!(
            argv("flaky", json!({"since": "14d", "repo": "a/b"})),
            ["flaky", "--repo", "a/b", "--since", "14d"]
        );
    }
}
