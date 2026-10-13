//! yuzu's MCP tools: each maps its arguments to a yuzu command line. The
//! hook isn't one: safety checks run as hooks, never on the agent's say-so
//! (docs/yuzu/agents.md).

use mochi_common::mcp::{Args, Server, Tool};
use serde_json::{Value, json};

pub const INSTRUCTIONS: &str = "yuzu answers what your environment is: which cluster, cloud account, host, and remote the shell points at and whose they are (status), what a command would touch and whether it's allowed (check), and whether a URL works from where the user is, not just from your shell (probe). When check says a command needs approval, ask the user; don't look for another way to run it.";

fn status(_: &Value) -> Result<Vec<String>, String> {
    Ok(vec!["status".into()])
}

fn check(v: &Value) -> Result<Vec<String>, String> {
    Ok(vec![
        "check".into(),
        "--".into(),
        Args(v).required("command")?,
    ])
}

fn wait(v: &Value) -> Result<Vec<String>, String> {
    let a = Args(v);
    let mut argv = vec![
        "wait".to_owned(),
        "--timeout".to_owned(),
        a.str("timeout")?.unwrap_or_else(|| "5m".into()),
    ];
    a.push_opt(&mut argv, "every", "--every")?;
    if a.flag("not") {
        argv.push("--not".into());
    }
    let what = a.required("condition")?;
    let target = a.str("target")?;
    match what.as_str() {
        "ci" => {
            argv.push("ci".into());
            argv.extend(target);
        }
        "port" | "url" | "file" => {
            argv.push(what);
            argv.push(target.ok_or("`target` is required: host:port, a URL, or a path")?);
        }
        "cmd" => {
            argv.extend(["cmd".into(), "--".into()]);
            argv.push(target.ok_or("`target` is required: the command line")?);
        }
        other => {
            return Err(format!(
                "`condition` is ci, port, url, file, or cmd, not {other:?}"
            ));
        }
    }
    Ok(argv)
}

fn whoami(v: &Value) -> Result<Vec<String>, String> {
    let mut argv = vec!["whoami".to_owned()];
    if Args(v).flag("verify") {
        argv.push("--verify".into());
    }
    Ok(argv)
}

fn probe(v: &Value) -> Result<Vec<String>, String> {
    let a = Args(v);
    let mut argv = vec!["probe".to_owned()];
    a.push_opt(&mut argv, "browser", "--browser")?;
    a.push_opt(&mut argv, "timeout", "--timeout")?;
    argv.push(a.required("url")?);
    Ok(argv)
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "status",
            description: "Use before acting on a cluster, cloud account, or host: the current kube context, AWS profile, and git remotes, and which declared environment (mine, work, production) each belongs to.",
            input_schema: json!({ "type": "object", "properties": {} }),
            args: status,
        },
        Tool {
            name: "check",
            description: "Use before running a command that might change a cluster, account, host, or remote: lists what it would touch and whether it's allowed, needs the user's approval, or is denied.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The shell command line, as you'd run it." },
                },
                "required": ["command"],
            }),
            args: check,
        },
        Tool {
            name: "wait",
            description: "Use instead of a sleep or poll loop whenever you're waiting on something: CI for a PR or commit to finish (the result says pass or fail), a port to open, a URL to answer, a file to appear, or a command to succeed. Returns when it's done, failed, or timed out, with what it last saw.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "condition": { "type": "string", "enum": ["ci", "port", "url", "file", "cmd"] },
                    "target": { "type": "string", "description": "ci: a PR number or ref (default HEAD); port: host:port; url: the URL; file: a path; cmd: a shell command line." },
                    "timeout": { "type": "string", "description": "Give up after this long: 90s, 10m (default 5m; keep it under your tool-call timeout)." },
                    "every": { "type": "string", "description": "How often to look." },
                    "not": { "type": "boolean", "description": "Wait until it's no longer true." },
                },
                "required": ["condition"],
            }),
            args: wait,
        },
        Tool {
            name: "whoami",
            description: "Use before pushing, opening a PR, or calling a cloud API, and whenever auth fails: which account gh, git, ssh, and AWS will act as here, where each comes from, and what overrides it (a GH_TOKEN that hides your login, AWS keys that hide a profile). With `verify`, asks each service.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "verify": { "type": "boolean", "description": "Ask GitHub, the git hosts, and AWS who you are (network calls)." },
                },
            }),
            args: whoami,
        },
        Tool {
            name: "probe",
            description: "Use when something works from your shell but the user says it doesn't, before asking them to debug: loads the URL with curl and with the user's browser (headless, empty profile) and names the likely cause, such as macOS Local Network permission or a CA only curl trusts.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "The http or https URL the user opens." },
                    "browser": { "type": "string", "description": "Path to a Chromium-based browser. Default: the first installed." },
                    "timeout": { "type": "integer", "description": "Seconds per load (default 15)." },
                },
                "required": ["url"],
            }),
            args: probe,
        },
    ]
}

pub fn serve() -> u8 {
    let tools = tools();
    mochi_common::mcp::serve(&Server {
        name: "yuzu",
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
        let run = |name: &str, v: Value| (tools.iter().find(|t| t.name == name).unwrap().args)(&v);
        assert_eq!(
            run("check", json!({"command": "kubectl delete ns x"})).unwrap(),
            ["check", "--", "kubectl delete ns x"]
        );
        assert_eq!(
            run("probe", json!({"url": "https://a.local/", "timeout": 5})).unwrap(),
            ["probe", "--timeout", "5", "https://a.local/"]
        );
        assert_eq!(
            run("check", json!({})).unwrap_err(),
            "`command` is required"
        );
        assert_eq!(
            run("wait", json!({"condition": "ci", "target": "2"})).unwrap(),
            ["wait", "--timeout", "5m", "ci", "2"]
        );
        assert_eq!(
            run("wait", json!({"condition": "cmd", "target": "kubectl rollout status deploy/api", "timeout": "10m"})).unwrap(),
            ["wait", "--timeout", "10m", "cmd", "--", "kubectl rollout status deploy/api"]
        );
        assert!(
            run("wait", json!({"condition": "port"}))
                .unwrap_err()
                .contains("host:port")
        );
    }
}
