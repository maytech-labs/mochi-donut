//! matcha's MCP tools: each maps its arguments to a matcha command line.
//! Descriptions are triggers, written for the agent deciding what to call
//! (docs/design.md, "How agents find the tools").

use mochi_common::mcp::{Args, Server, Tool};
use serde_json::{Value, json};

pub const INSTRUCTIONS: &str = "matcha answers what this repo knows, with evidence: how it usually does something (census), what else changes with a change (recipe), whether its docs are still true (drift), and what you verified versus assumed (ledger). Every result is JSON with counts and examples; read them, don't round them up. `repo` defaults to the server's working directory.";

fn repo_prop() -> Value {
    json!({ "type": "string", "description": "The repo's path. Default: the server's working directory." })
}

fn base(a: &Args, cmd: &[&str]) -> Result<Vec<String>, String> {
    let mut argv: Vec<String> = Vec::new();
    a.push_opt(&mut argv, "repo", "--repo")?;
    argv.extend(cmd.iter().map(|s| (*s).to_owned()));
    Ok(argv)
}

fn census(v: &Value) -> Result<Vec<String>, String> {
    let a = Args(v);
    let mut argv = base(&a, &["census"])?;
    if let Some(label) = a.str("label")? {
        argv.push(label);
    }
    a.push_each(&mut argv, "alts", "--alt")?;
    a.push_opt(&mut argv, "spelling", "--spelling")?;
    a.push_each(&mut argv, "in", "--in")?;
    a.push_opt(&mut argv, "examples", "--examples")?;
    Ok(argv)
}

fn recipe_check(v: &Value) -> Result<Vec<String>, String> {
    let a = Args(v);
    let mut argv = base(&a, &["recipe", "--check"])?;
    a.push_opt(&mut argv, "commit", "--commit")?;
    Ok(argv)
}

fn recipe_like(v: &Value) -> Result<Vec<String>, String> {
    let a = Args(v);
    let mut argv = base(&a, &["recipe", "--like"])?;
    argv.push(a.required("text")?);
    Ok(argv)
}

fn drift(v: &Value) -> Result<Vec<String>, String> {
    let a = Args(v);
    let mut argv = base(&a, &["drift"])?;
    a.push_each(&mut argv, "in", "--in")?;
    if a.flag("suggest") {
        argv.push("--suggest".into());
    }
    Ok(argv)
}

fn evidence(a: &Args, argv: &mut Vec<String>) -> Result<(), String> {
    a.push_opt(argv, "evidence", "--evidence")?;
    a.push_opt(argv, "file", "--file")?;
    a.push_opt(argv, "cmd", "--cmd")
}

fn ledger_add(v: &Value) -> Result<Vec<String>, String> {
    let a = Args(v);
    let status = a.required("status")?;
    if status != "verified" && status != "assumed" {
        return Err("`status` is `verified` or `assumed`".into());
    }
    let mut argv = base(&a, &["ledger", "add"])?;
    argv.push(status);
    argv.push(a.required("claim")?);
    evidence(&a, &mut argv)?;
    Ok(argv)
}

fn ledger_verify(v: &Value) -> Result<Vec<String>, String> {
    let a = Args(v);
    let mut argv = base(&a, &["ledger", "verify"])?;
    argv.push(a.required("id")?);
    evidence(&a, &mut argv)?;
    Ok(argv)
}

fn ledger_list(v: &Value) -> Result<Vec<String>, String> {
    base(&Args(v), &["ledger", "list"])
}

fn ledger_check(v: &Value) -> Result<Vec<String>, String> {
    let a = Args(v);
    let mut argv = base(&a, &["ledger", "check", "--diff"])?;
    a.push_opt(&mut argv, "base", "--base")?;
    Ok(argv)
}

fn ledger_summary(v: &Value) -> Result<Vec<String>, String> {
    base(&Args(v), &["ledger", "summary"])
}

fn evidence_props() -> Value {
    json!({
        "evidence": { "type": "string", "description": "What shows it: the output you saw, a URL." },
        "file": { "type": "string", "description": "A file and optional line that shows it: src/a.rs:12." },
        "cmd": { "type": "string", "description": "Text from a command you ran; attaches the latest recorded match and its output. Needs the matcha hook." },
        "repo": repo_prop(),
    })
}

fn with(mut props: Value, extra: Value) -> Value {
    if let (Some(p), Some(e)) = (props.as_object_mut(), extra.as_object()) {
        p.extend(e.clone());
    }
    props
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "census",
            description: "Use before copying a pattern, choosing between two ways of doing something, or writing a guideline: counts how often this repo does each alternative, by language, with the most typical files. Give each alternative as a regex in `alts`, or a word stem in `spelling` to see which spelling the repo uses.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "label": { "type": "string", "description": "What you're asking about, echoed back." },
                    "alts": { "type": "array", "items": { "type": "string" }, "description": "One regex per alternative." },
                    "spelling": { "type": "string", "description": "A word stem: counts every spelling that starts with it." },
                    "in": { "type": "array", "items": { "type": "string" }, "description": "Only look under these paths." },
                    "examples": { "type": "integer", "description": "Examples per alternative (default 3)." },
                    "repo": repo_prop(),
                },
            }),
            args: census,
        },
        Tool {
            name: "recipe_like",
            description: "Use before a change you haven't made in this repo (add an endpoint, a migration, a setting): finds past commits like it and lists the files they changed together, so you know the companion edits up front.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "What you're doing, in commit-subject words: \"add an endpoint\"." },
                    "repo": repo_prop(),
                },
                "required": ["text"],
            }),
            args: recipe_like,
        },
        Tool {
            name: "recipe_check",
            description: "Use before committing: names files that usually change with the ones you changed but didn't, with the counts from history and the usual fix (a make target or npm script).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "commit": { "type": "string", "description": "Check this past commit instead of the working tree." },
                    "repo": repo_prop(),
                },
            }),
            args: recipe_check,
        },
        Tool {
            name: "drift",
            description: "Use before trusting a doc's claims about the code, and after changing code a doc describes: lists doc links, paths, names, make targets, and check comments that no longer match the repo. With `suggest`, proposes check comments for rule sentences (\"never call X\") with whether the code follows them today.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "in": { "type": "array", "items": { "type": "string" }, "description": "Only check docs under these paths." },
                    "suggest": { "type": "boolean", "description": "Propose check comments instead of checking." },
                    "repo": repo_prop(),
                },
            }),
            args: drift,
        },
        Tool {
            name: "ledger_add",
            description: "Use whenever you check a fact or have to assume one, before you write it into a doc, comment, or PR: records the claim as `verified` (evidence required) or `assumed`. Decisions go in ADRs; facts go here.",
            input_schema: json!({
                "type": "object",
                "properties": with(json!({
                    "status": { "type": "string", "enum": ["verified", "assumed"] },
                    "claim": { "type": "string", "description": "The claim, in one sentence." },
                }), evidence_props()),
                "required": ["status", "claim"],
            }),
            args: ledger_add,
        },
        Tool {
            name: "ledger_verify",
            description: "Use when you've checked a claim you recorded as assumed: marks it verified, with the evidence.",
            input_schema: json!({
                "type": "object",
                "properties": with(json!({
                    "id": { "type": "integer", "description": "The entry's number from ledger_list." },
                }), evidence_props()),
                "required": ["id"],
            }),
            args: ledger_verify,
        },
        Tool {
            name: "ledger_list",
            description: "Use to see what this branch has verified and assumed so far.",
            input_schema: json!({ "type": "object", "properties": { "repo": repo_prop() } }),
            args: ledger_list,
        },
        Tool {
            name: "ledger_check",
            description: "Use before committing docs or comments: flags added lines that state a claim you recorded as assumed.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "base": { "type": "string", "description": "Diff against this commit or branch (default HEAD)." },
                    "repo": repo_prop(),
                },
            }),
            args: ledger_check,
        },
        Tool {
            name: "ledger_summary",
            description: "Use when writing a PR description: the branch's claims as Markdown, verified (with evidence) and assumed.",
            input_schema: json!({ "type": "object", "properties": { "repo": repo_prop() } }),
            args: ledger_summary,
        },
    ]
}

pub fn serve() -> u8 {
    let tools = tools();
    mochi_common::mcp::serve(&Server {
        name: "matcha",
        version: env!("CARGO_PKG_VERSION"),
        instructions: INSTRUCTIONS,
        tools: &tools,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(name: &str, v: Value) -> Result<Vec<String>, String> {
        let tools = tools();
        let tool = tools.iter().find(|t| t.name == name).unwrap();
        (tool.args)(&v)
    }

    #[test]
    fn maps_arguments_to_command_lines() {
        assert_eq!(
            argv("census", json!({"label": "forms", "alts": ["useForm\\(", "Draft"], "in": ["web"], "repo": "/r"})).unwrap(),
            ["--repo", "/r", "census", "forms", "--alt", "useForm\\(", "--alt", "Draft", "--in", "web"]
        );
        assert_eq!(
            argv(
                "ledger_add",
                json!({"status": "verified", "claim": "c", "file": "a.rs:3"})
            )
            .unwrap(),
            ["ledger", "add", "verified", "c", "--file", "a.rs:3"]
        );
        assert_eq!(
            argv("ledger_verify", json!({"id": 2, "evidence": "saw it"})).unwrap(),
            ["ledger", "verify", "2", "--evidence", "saw it"]
        );
        assert_eq!(
            argv("drift", json!({"suggest": true})).unwrap(),
            ["drift", "--suggest"]
        );
        assert_eq!(
            argv("ledger_check", json!({})).unwrap(),
            ["ledger", "check", "--diff"]
        );
    }

    #[test]
    fn rejects_bad_arguments_with_a_reason() {
        assert_eq!(
            argv("recipe_like", json!({})).unwrap_err(),
            "`text` is required"
        );
        assert!(
            argv("ledger_add", json!({"status": "sure", "claim": "c"}))
                .unwrap_err()
                .contains("verified")
        );
        assert!(
            argv("census", json!({"alts": 3}))
                .unwrap_err()
                .contains("list of strings")
        );
    }

    #[test]
    fn every_tool_says_when_to_use_it() {
        for t in tools() {
            assert!(t.description.starts_with("Use "), "{}", t.name);
            assert_eq!(t.input_schema["type"], "object", "{}", t.name);
        }
    }
}
