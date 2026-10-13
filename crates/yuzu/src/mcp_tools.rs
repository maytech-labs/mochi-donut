//! yuzu's MCP tools: each maps its arguments to a yuzu command line. The
//! hook isn't one: safety checks run as hooks, never on the agent's say-so
//! (docs/design.md, "How agents find the tools").

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
    }
}
