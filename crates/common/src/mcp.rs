//! A Model Context Protocol server over stdio, as a thin layer over a tool's
//! own CLI: each MCP tool call runs the binary again with `--json` and hands
//! back what it printed (docs/design.md, "Interfaces";
//! docs/decisions/0019-mcp-and-plugin.md).

use std::io::{BufRead, Write};
use std::process::Command;

use serde_json::{Value, json};

/// Protocol versions this server speaks, newest first.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// One MCP tool, backed by a CLI command line.
pub struct Tool {
    pub name: &'static str,
    /// Written as a trigger ("Use before ..."), since it's what makes an
    /// agent reach for the tool.
    pub description: &'static str,
    pub input_schema: Value,
    /// The CLI arguments for a call, without `--json`, which is put first.
    pub args: fn(&Value) -> Result<Vec<String>, String>,
}

pub struct Server<'a> {
    pub name: &'static str,
    pub version: &'static str,
    /// Shown to the agent when it connects.
    pub instructions: &'static str,
    pub tools: &'a [Tool],
}

/// Serves MCP on stdin and stdout until stdin closes. Returns the exit code.
pub fn serve(server: &Server) -> u8 {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handle(server, &line, &run_self)
            && (writeln!(stdout, "{reply}").is_err() || stdout.flush().is_err())
        {
            break;
        }
    }
    crate::exit::OK
}

/// What running the CLI produced.
pub struct Ran {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

fn run_self(args: &[String]) -> Result<Ran, String> {
    let exe = std::env::current_exe().map_err(|e| format!("couldn't find this binary: {e}"))?;
    // `--json` goes first: after a `--`, it would be part of the command
    // `yuzu check` is asked about.
    let out = Command::new(exe)
        .arg("--json")
        .args(args)
        .output()
        .map_err(|e| format!("couldn't run this binary: {e}"))?;
    Ok(Ran {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// Answers one JSON-RPC message, or `None` for a notification.
pub fn handle(
    server: &Server,
    line: &str,
    run: &dyn Fn(&[String]) -> Result<Ran, String>,
) -> Option<String> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
    };
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    // Notifications (no id) get no reply.
    let id = id?;
    let result = match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let version = asked
                .filter(|v| PROTOCOL_VERSIONS.contains(v))
                .unwrap_or(PROTOCOL_VERSIONS[0]);
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": server.name, "version": server.version },
                "instructions": server.instructions,
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({
            "tools": server.tools.iter().map(|t| json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema,
            })).collect::<Vec<_>>()
        }),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let Some(tool) = server.tools.iter().find(|t| t.name == name) else {
                return Some(error(id, -32602, &format!("unknown tool {name:?}")));
            };
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            call(tool, &args, run)
        }
        other => return Some(error(id, -32601, &format!("method not found: {other}"))),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string())
}

/// Runs a tool. Exit 0 and 1 are both answers (1 means a check found
/// something); anything else is an error the agent should read.
fn call(tool: &Tool, args: &Value, run: &dyn Fn(&[String]) -> Result<Ran, String>) -> Value {
    let argv = match (tool.args)(args) {
        Ok(a) => a,
        Err(e) => return text_result(&e, true),
    };
    match run(&argv) {
        Ok(ran) if matches!(ran.code, Some(0 | 1)) => text_result(ran.stdout.trim(), false),
        Ok(ran) => {
            let msg = [ran.stderr.trim(), ran.stdout.trim()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            text_result(&msg, true)
        }
        Err(e) => text_result(&e, true),
    }
}

fn text_result(text: &str, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

fn error(id: Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

/// Reads tool arguments into CLI flags.
pub struct Args<'a>(pub &'a Value);

impl Args<'_> {
    pub fn str(&self, key: &str) -> Result<Option<String>, String> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(Value::Number(n)) => Ok(Some(n.to_string())),
            Some(other) => Err(format!("`{key}` should be a string, not {other}")),
        }
    }

    pub fn required(&self, key: &str) -> Result<String, String> {
        self.str(key)?
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("`{key}` is required"))
    }

    pub fn list(&self, key: &str) -> Result<Vec<String>, String> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::String(s)) => Ok(vec![s.clone()]),
            Some(Value::Array(items)) => items
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| format!("`{key}` should hold strings"))
                })
                .collect(),
            Some(other) => Err(format!("`{key}` should be a list of strings, not {other}")),
        }
    }

    pub fn flag(&self, key: &str) -> bool {
        self.0.get(key).and_then(Value::as_bool).unwrap_or(false)
    }

    /// Appends `--flag value` when `key` is set.
    pub fn push_opt(&self, argv: &mut Vec<String>, key: &str, flag: &str) -> Result<(), String> {
        if let Some(v) = self.str(key)? {
            argv.push(flag.to_owned());
            argv.push(v);
        }
        Ok(())
    }

    /// Appends `--flag value` for each item of a list.
    pub fn push_each(&self, argv: &mut Vec<String>, key: &str, flag: &str) -> Result<(), String> {
        for v in self.list(key)? {
            argv.push(flag.to_owned());
            argv.push(v);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo_args(v: &Value) -> Result<Vec<String>, String> {
        let a = Args(v);
        let mut argv = vec!["echo".to_owned(), a.required("text")?];
        a.push_each(&mut argv, "in", "--in")?;
        Ok(argv)
    }

    const TOOLS: &[Tool] = &[];

    fn server(tools: &[Tool]) -> Server<'_> {
        Server {
            name: "test",
            version: "0.0.0",
            instructions: "Use it.",
            tools,
        }
    }

    fn fake_run(args: &[String]) -> Result<Ran, String> {
        let code = if args.iter().any(|a| a == "bad") {
            2
        } else {
            1
        };
        Ok(Ran {
            code: Some(code),
            stdout: format!("{{\"args\":{:?}}}", args),
            stderr: "usage: try again".into(),
        })
    }

    fn reply(server: &Server, msg: Value) -> Value {
        serde_json::from_str(&handle(server, &msg.to_string(), &fake_run).unwrap()).unwrap()
    }

    #[test]
    fn negotiates_the_protocol_version() {
        let s = server(TOOLS);
        let r = reply(
            &s,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}),
        );
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(r["result"]["serverInfo"]["name"], "test");
        let r = reply(
            &s,
            json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}),
        );
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[test]
    fn notifications_get_no_reply_and_unknown_methods_an_error() {
        let s = server(TOOLS);
        assert!(
            handle(
                &s,
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                &fake_run
            )
            .is_none()
        );
        let r = reply(
            &s,
            json!({"jsonrpc":"2.0","id":3,"method":"resources/list"}),
        );
        assert_eq!(r["error"]["code"], -32601);
        let r: Value = serde_json::from_str(&handle(&s, "{oops", &fake_run).unwrap()).unwrap();
        assert_eq!(r["error"]["code"], -32700);
    }

    #[test]
    fn calls_run_the_cli_and_pass_check_failures_through() {
        let tools = [Tool {
            name: "echo",
            description: "Use to echo.",
            input_schema: json!({"type":"object"}),
            args: echo_args,
        }];
        let s = server(&tools);
        let list = reply(&s, json!({"jsonrpc":"2.0","id":4,"method":"tools/list"}));
        assert_eq!(list["result"]["tools"][0]["name"], "echo");

        let r = reply(
            &s,
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"echo","arguments":{"text":"hi","in":["a","b"]}}}),
        );
        assert_eq!(r["result"]["isError"], false);
        assert_eq!(
            r["result"]["content"][0]["text"],
            r#"{"args":["echo", "hi", "--in", "a", "--in", "b"]}"#
        );

        let r = reply(
            &s,
            json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"echo","arguments":{"text":"bad"}}}),
        );
        assert_eq!(r["result"]["isError"], true);
        assert!(
            r["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("usage: try again")
        );

        let r = reply(
            &s,
            json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"echo","arguments":{}}}),
        );
        assert_eq!(r["result"]["isError"], true);
        assert_eq!(r["result"]["content"][0]["text"], "`text` is required");

        let r = reply(
            &s,
            json!({"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"nope"}}),
        );
        assert_eq!(r["error"]["code"], -32602);
    }
}
