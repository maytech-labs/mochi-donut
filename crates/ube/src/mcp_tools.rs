//! ube's MCP tools: each maps its arguments to a ube command line
//! (docs/ube/agents.md).

use mochi_common::mcp::{Args, Server, Tool};
use serde_json::{Value, json};

pub const INSTRUCTIONS: &str = "ube knows the change in hand: which of the repo's checks apply to it, whether they passed on this exact code, and what failed, in a few lines. Run `done` before saying a change is done; if it says checks have not run, use `run`. `repo` defaults to the server's working directory.";

fn common(a: &Args, cmd: &str) -> Result<Vec<String>, String> {
    let mut argv = Vec::new();
    a.push_opt(&mut argv, "repo", "--repo")?;
    argv.push(cmd.to_owned());
    a.push_opt(&mut argv, "base", "--base")?;
    if a.flag("all") {
        argv.push("--all".into());
    }
    Ok(argv)
}

fn plan(v: &Value) -> Result<Vec<String>, String> {
    common(&Args(v), "plan")
}

fn run(v: &Value) -> Result<Vec<String>, String> {
    let a = Args(v);
    let mut argv = common(&a, "run")?;
    if a.flag("rerun_failed") {
        argv.push("--rerun-failed".into());
    }
    if let Some(c) = a.str("command")? {
        argv.extend(["--".into(), c]);
    }
    Ok(argv)
}

fn done(v: &Value) -> Result<Vec<String>, String> {
    let a = Args(v);
    let mut argv = Vec::new();
    a.push_opt(&mut argv, "repo", "--repo")?;
    argv.push("done".into());
    a.push_opt(&mut argv, "base", "--base")?;
    Ok(argv)
}

fn props(extra: Value) -> Value {
    let mut p = json!({
        "repo": { "type": "string", "description": "The repo's path. Default: the server's working directory." },
        "base": { "type": "string", "description": "Compare against this commit or branch instead of HEAD." },
    });
    if let (Some(p), Some(e)) = (p.as_object_mut(), extra.as_object()) {
        p.extend(e.clone());
    }
    p
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "plan",
            description: "Use before testing a change: which of the repo's own checks apply to the changed files (per package, plus packages that depend on them), why each, and what's left out.",
            input_schema: json!({ "type": "object", "properties": props(json!({ "all": { "type": "boolean", "description": "Plan the full suite." } })) }),
            args: plan,
        },
        Tool {
            name: "run",
            description: "Use to test a change instead of running test commands yourself: runs the planned checks (or one `command`) and returns each failure in a few lines with a command to rerun just it. Results are tied to the exact code they ran on.",
            input_schema: json!({ "type": "object", "properties": props(json!({
                "all": { "type": "boolean", "description": "Run the full suite." },
                "rerun_failed": { "type": "boolean", "description": "Rerun only what failed last time on this code." },
                "command": { "type": "string", "description": "Run this one command instead of the plan." },
            })) }),
            args: run,
        },
        Tool {
            name: "done",
            description: "Use before saying a change is done: a checklist for the exact current code. Planned checks passed on it, changed code has test changes, no companion file is missing, no doc claim it made false, no assumed claim written into a doc.",
            input_schema: json!({ "type": "object", "properties": props(json!({})) }),
            args: done,
        },
    ]
}

pub fn serve() -> u8 {
    let tools = tools();
    mochi_common::mcp::serve(&Server {
        name: "ube",
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
        assert_eq!(
            argv("plan", json!({"base": "main"})),
            ["plan", "--base", "main"]
        );
        assert_eq!(
            argv("run", json!({"command": "cargo test -p x"})),
            ["run", "--", "cargo test -p x"]
        );
        assert_eq!(
            argv("run", json!({"rerun_failed": true, "repo": "/r"})),
            ["--repo", "/r", "run", "--rerun-failed"]
        );
        assert_eq!(argv("done", json!({})), ["done"]);
    }
}
