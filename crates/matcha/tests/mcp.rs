//! `matcha mcp`, spoken to as an MCP client would, over stdio.

// Test helpers may panic; clippy's allow-unwrap-in-tests only covers #[test] fns.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::Command as Process;

use assert_cmd::Command;
use serde_json::{Value, json};

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let ok = Process::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .status()
            .unwrap()
            .success();
        assert!(ok);
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(
        dir.path().join("README.md"),
        "# Demo\n\nSee [docs](docs/gone.md).\n",
    )
    .unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "init"]);
    dir
}

/// Sends `messages` to `matcha mcp` running in `cwd` and returns its replies.
fn session(cwd: &Path, messages: &[Value]) -> Vec<Value> {
    let input: String = messages.iter().map(|m| format!("{m}\n")).collect();
    let out = Command::cargo_bin("matcha")
        .unwrap()
        .arg("mcp")
        .current_dir(cwd)
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn call(id: u32, name: &str, arguments: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": arguments}})
}

#[test]
fn an_agent_can_list_and_call_the_tools() {
    let repo = repo();
    let replies = session(
        repo.path(),
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            call(
                3,
                "ledger_add",
                json!({"status": "assumed", "claim": "The demo has docs"}),
            ),
            call(4, "ledger_list", json!({})),
            call(5, "drift", json!({})),
            call(
                6,
                "ledger_add",
                json!({"status": "verified", "claim": "no evidence"}),
            ),
        ],
    );
    // The notification gets no reply.
    assert_eq!(replies.len(), 6);
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "matcha");
    let names: Vec<&str> = replies[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "census",
            "recipe_like",
            "recipe_check",
            "drift",
            "ledger_add",
            "ledger_verify",
            "ledger_list",
            "ledger_check",
            "ledger_summary"
        ]
    );

    let text = |i: usize| -> Value {
        serde_json::from_str(replies[i]["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    assert_eq!(text(2)["entry"]["claim"], "The demo has docs");
    assert_eq!(text(3)["entries"][0]["status"], "assumed");
    // drift found the broken link: exit 1 is an answer, not an error.
    assert_eq!(replies[4]["result"]["isError"], false);
    assert_eq!(text(4)["failing"][0]["claim"], "docs/gone.md");
    // A usage error comes back as a tool error the agent can read.
    assert_eq!(replies[5]["result"]["isError"], true);
    assert!(
        replies[5]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("needs evidence")
    );
}
