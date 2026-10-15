//! The yuzu binary, run as an agent or a hook would run it, in a temporary
//! home with its own kubeconfig and declarations.

// Test helpers may panic; clippy's allow-unwrap-in-tests only covers #[test] fns.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

const ENVIRONMENTS: &str = r#"
[[env]]
name = "homelab"
class = "mine"
kube_contexts = ["k3s-pis"]
ssh_hosts = ["samwise", "merry", "pippin"]

[[env]]
name = "payments"
class = "production"
agents = "none"
kube_contexts = ["*-payments-*"]

[[env]]
name = "work"
class = "work"
kube_contexts = ["work-*"]
"#;

/// A home whose kubeconfig points at a work cluster, the way this Mac's did.
fn home(environments: Option<&str>) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".kube")).unwrap();
    std::fs::write(
        home.path().join(".kube/config"),
        "apiVersion: v1\ncurrent-context: work-nonprod-us-east-1\nkind: Config\n",
    )
    .unwrap();
    if let Some(text) = environments {
        let dir = home.path().join(".config/yuzu");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("environments.toml"), text).unwrap();
    }
    home
}

fn yuzu(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("yuzu").unwrap();
    cmd.current_dir(home)
        .env("HOME", home)
        .env_remove("KUBECONFIG")
        .env_remove("YUZU_CONFIG")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_STATE_HOME")
        .env_remove("YUZU_NO_LOG")
        .env_remove("AWS_PROFILE")
        .env_remove("AWS_DEFAULT_PROFILE");
    cmd
}

fn json(cmd: &mut Command, code: i32) -> Value {
    let out = cmd
        .arg("--json")
        .assert()
        .code(code)
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).unwrap()
}

fn check(home: &Path, line: &str) -> (i32, Value) {
    let out = yuzu(home)
        .args(["--json", "check", "--", line])
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        serde_json::from_slice(&out.stdout).unwrap(),
    )
}

fn hook(home: &Path, input: &str) -> String {
    let out = yuzu(home)
        .args(["hook", "claude-code"])
        .write_stdin(input)
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap()
}

fn bash(command: &str) -> String {
    serde_json::json!({
        "tool_name": "Bash",
        "tool_input": { "command": command },
        "cwd": "/",
    })
    .to_string()
}

#[test]
fn prints_its_version() {
    let h = home(None);
    yuzu(h.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("yuzu "));
}

#[test]
fn agent_guide_says_when_to_run_what() {
    let h = home(None);
    yuzu(h.path())
        .arg("agent-guide")
        .assert()
        .success()
        .stdout(predicate::str::contains("When to run what"));
}

#[test]
fn status_is_the_default_and_says_whose_it_is() {
    let h = home(Some(ENVIRONMENTS));
    let body = json(&mut yuzu(h.path()), 0);
    assert_eq!(body["config"]["environments"], 3);
    let kube = &body["targets"][0];
    assert_eq!(kube["kind"], "kube_context");
    assert_eq!(kube["name"], "work-nonprod-us-east-1");
    assert_eq!(kube["env"], "work");
    assert_eq!(kube["agents"], "read-only");

    yuzu(h.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("work (work, agents read-only)"));
}

// The design's bar: with the homelab declared, a write against any other
// context is stopped before it runs, and reads still work.
#[test]
fn writes_outside_the_homelab_need_approval_and_reads_pass() {
    let h = home(Some(ENVIRONMENTS));

    let (code, body) = check(h.path(), "kubectl apply -f openbao.yaml");
    assert_eq!(code, 1);
    assert_eq!(body["decision"], "ask");
    assert_eq!(body["targets"][0]["name"], "work-nonprod-us-east-1");
    let message = body["message"].as_str().unwrap();
    assert!(message.contains("--context k3s-pis"), "{message}");

    let (code, body) = check(h.path(), "kubectl get pods -A");
    assert_eq!((code, &body["decision"]), (0, &Value::from("allow")));

    let (code, _) = check(h.path(), "kubectl --context k3s-pis apply -f openbao.yaml");
    assert_eq!(code, 0);
    let (code, _) = check(
        h.path(),
        "ssh samwise 'sudo -n k3s kubectl apply -f x.yaml'",
    );
    assert_eq!(code, 0);
    let (code, _) = check(h.path(), "kubectl --context somewhere-new delete ns x");
    assert_eq!(code, 1);
}

#[test]
fn finds_the_write_inside_a_longer_line() {
    let h = home(Some(ENVIRONMENTS));
    let (code, body) = check(
        h.path(),
        "cd /tmp && kubectl --context k3s-pis get pods | head; echo \"$(kubectl delete ns x)\"",
    );
    assert_eq!(code, 1);
    assert_eq!(body["targets"].as_array().unwrap().len(), 2);
    assert_eq!(body["targets"][1]["access"], "write");
}

#[test]
fn several_words_are_one_command() {
    let h = home(Some(ENVIRONMENTS));
    yuzu(h.path())
        .args(["check", "--", "kubectl", "delete", "pod", "x"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("NEEDS APPROVAL"));
    yuzu(h.path())
        .args(["check", "--", "ls", "-la"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("Allowed"));
    yuzu(h.path()).arg("check").assert().code(2);
}

#[test]
fn agents_none_denies_even_reads() {
    let h = home(Some(ENVIRONMENTS));
    let (code, body) = check(h.path(), "kubectl --context prod-payments-1 get pods");
    assert_eq!(code, 1);
    assert_eq!(body["decision"], "deny");
}

#[test]
fn with_nothing_declared_writes_need_approval() {
    let h = home(None);
    let (code, body) = check(h.path(), "kubectl apply -f x.yaml");
    assert_eq!(code, 1);
    assert_eq!(body["decision"], "ask");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("No environments are declared")
    );
    let (code, _) = check(h.path(), "kubectl get pods");
    assert_eq!(code, 0);
}

#[test]
fn a_broken_config_blocks_instead_of_allowing() {
    let h = home(Some(
        "[[env]]\nname = \"x\"\nclass = \"mine\"\nkube_context = \"*\"\n",
    ));
    let (code, body) = check(h.path(), "ls");
    assert_eq!(code, 1);
    assert!(body["message"].as_str().unwrap().contains("kube_context"));
    assert!(hook(h.path(), &bash("ls")).contains("\"permissionDecision\":\"ask\""));
}

#[test]
fn the_hook_is_silent_when_allowed_and_asks_otherwise() {
    let h = home(Some(ENVIRONMENTS));
    assert_eq!(hook(h.path(), &bash("kubectl get pods")), "");
    assert_eq!(hook(h.path(), &bash("git status && ls")), "");

    let out: Value =
        serde_json::from_str(&hook(h.path(), &bash("kubectl apply -f x.yaml"))).unwrap();
    let decision = &out["hookSpecificOutput"];
    assert_eq!(decision["hookEventName"], "PreToolUse");
    assert_eq!(decision["permissionDecision"], "ask");
    let reason = decision["permissionDecisionReason"].as_str().unwrap();
    assert!(
        reason.starts_with("yuzu: `kubectl apply -f x.yaml` would change kube context"),
        "{reason}"
    );

    let out = hook(
        h.path(),
        &bash("kubectl --context prod-payments-1 get pods"),
    );
    assert!(out.contains("\"permissionDecision\":\"deny\""));
}

#[test]
fn the_hook_ignores_other_tools_and_asks_on_bad_input() {
    let h = home(Some(ENVIRONMENTS));
    let read = r#"{"tool_name":"Read","tool_input":{"file_path":"/x"}}"#;
    assert_eq!(hook(h.path(), read), "");
    assert!(hook(h.path(), "not json").contains("\"permissionDecision\":\"ask\""));
}

// The environments file decides what agents may change, so an agent that
// could edit it freely could approve itself.
#[test]
fn changing_yuzus_own_config_needs_approval() {
    let h = home(Some(ENVIRONMENTS));
    for line in [
        "sed -i '' 's/samwise/balin/' ~/.config/yuzu/environments.toml",
        "echo 'ssh_hosts = [\"*\"]' >> $HOME/.config/yuzu/environments.toml",
        "rm -rf ~/.config/yuzu",
    ] {
        let (code, body) = check(h.path(), line);
        assert_eq!(code, 1, "{line}");
        assert_eq!(body["decision"], "ask", "{line}");
        assert_eq!(body["targets"][0]["kind"], "yuzu_config", "{line}");
        let message = body["message"].as_str().unwrap();
        assert!(message.contains("yuzu's own config"), "{message}");
        assert!(message.contains("ask the user"), "{message}");
    }

    let (code, body) = check(h.path(), "cat ~/.config/yuzu/environments.toml");
    assert_eq!((code, &body["decision"]), (0, &Value::from("allow")));
    assert_eq!(body["targets"][0]["access"], "read");

    let out = hook(
        h.path(),
        &bash("sed -i '' 's/a/b/' ~/.config/yuzu/environments.toml"),
    );
    assert!(out.contains("\"permissionDecision\":\"ask\""), "{out}");
}

#[test]
fn env_add_declares_a_host_and_keeps_the_files_comments() {
    let h = home(Some(&format!("# my hosts\n{ENVIRONMENTS}")));
    let (code, body) = check(h.path(), "ssh balin docker start x");
    assert_eq!(code, 1);
    let message = body["message"].as_str().unwrap();
    assert!(
        message.contains("`yuzu env add homelab ssh_hosts balin`"),
        "{message}"
    );

    let body = json(
        yuzu(h.path()).args(["env", "add", "homelab", "ssh_hosts", "balin", "merry"]),
        0,
    );
    assert_eq!(body["env"], "homelab");
    assert_eq!(body["key"], "ssh_hosts");
    assert_eq!(body["created_env"], false);
    assert_eq!(body["added"], serde_json::json!(["balin"]));
    assert_eq!(body["already"], serde_json::json!(["merry"]));
    let text = std::fs::read_to_string(h.path().join(".config/yuzu/environments.toml")).unwrap();
    assert!(text.starts_with("# my hosts\n"), "{text}");

    let (code, body) = check(h.path(), "ssh balin docker start x");
    assert_eq!(code, 0);
    assert_eq!(body["targets"][0]["env"], "homelab");
}

#[test]
fn env_add_makes_an_env_only_with_a_class() {
    let h = home(None);
    yuzu(h.path())
        .args(["env", "add", "lab", "ssh_hosts", "pi-1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--class mine"));
    yuzu(h.path())
        .args(["env", "add", "lab", "ssh_host", "pi-1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("ssh_hosts"));

    let body = json(
        yuzu(h.path()).args(["env", "add", "lab", "ssh_hosts", "pi-1", "--class", "mine"]),
        0,
    );
    assert_eq!(body["created_env"], true);
    let (code, _) = check(h.path(), "ssh pi-1 reboot");
    assert_eq!(code, 0);
}

#[test]
fn env_add_says_when_an_earlier_env_still_wins() {
    let h = home(Some(ENVIRONMENTS));
    let body = json(
        yuzu(h.path()).args([
            "env",
            "add",
            "mine2",
            "kube_contexts",
            "work-sandbox",
            "--class",
            "mine",
        ]),
        0,
    );
    assert_eq!(body["shadowed"][0]["pattern"], "work-sandbox");
    assert_eq!(body["shadowed"][0]["env"], "work");
}

#[test]
fn the_hook_asks_before_a_file_tool_edits_yuzus_config() {
    let h = home(Some(ENVIRONMENTS));
    let config = h.path().join(".config/yuzu/environments.toml");
    for tool in ["Edit", "Write", "MultiEdit"] {
        let input = serde_json::json!({
            "tool_name": tool,
            "tool_input": { "file_path": config, "content": "" },
            "cwd": h.path(),
        })
        .to_string();
        let out = hook(h.path(), &input);
        assert!(
            out.contains("\"permissionDecision\":\"ask\""),
            "{tool}: {out}"
        );
        assert!(out.contains("yuzu's own config"), "{tool}: {out}");
    }

    let elsewhere = serde_json::json!({
        "tool_name": "Edit",
        "tool_input": { "file_path": h.path().join("notes.md") },
        "cwd": h.path(),
    })
    .to_string();
    assert_eq!(hook(h.path(), &elsewhere), "");
}

#[test]
fn mcp_serves_check_as_a_tool() {
    let h = home(Some(ENVIRONMENTS));
    let input = [
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"check","arguments":{"command":"kubectl delete ns payments"}}}),
    ]
    .iter()
    .map(|m| format!("{m}\n"))
    .collect::<String>();
    let out = yuzu(h.path())
        .arg("mcp")
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let replies: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "yuzu");
    let body: Value =
        serde_json::from_str(replies[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    // The work context needs approval for a write; that's an answer, not an error.
    assert_eq!(replies[1]["result"]["isError"], false);
    assert_eq!(body["decision"], "ask", "{body}");
}

#[test]
fn whoami_names_a_token_that_hides_the_stored_login() {
    let h = home(None);
    let gh = h.path().join("gh");
    std::fs::create_dir_all(&gh).unwrap();
    std::fs::write(
        gh.join("hosts.yml"),
        "github.com:\n    users:\n        me:\n    user: me\n",
    )
    .unwrap();
    let mut cmd = yuzu(h.path());
    cmd.env("GH_CONFIG_DIR", &gh)
        .env("GH_TOKEN", "stale")
        .env_remove("GITHUB_TOKEN")
        .env_remove("AWS_ACCESS_KEY_ID")
        .arg("whoami");
    let body = json(&mut cmd, 0);
    let github = body["identities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["scope"] == "github.com")
        .unwrap();
    assert_eq!(github["who"], "token from GH_TOKEN");
    assert!(
        github["warning"]
            .as_str()
            .unwrap()
            .contains("instead of me's stored login")
    );
    // Offline unless asked.
    assert_eq!(body["verified"], false);
    assert!(github.get("verified").is_none());
}

#[test]
fn wait_returns_when_met_and_exits_1_at_the_timeout() {
    let h = home(None);
    let ready = h.path().join("ready");
    std::fs::write(&ready, "").unwrap();
    let body = json(
        yuzu(h.path())
            .args(["wait", "--timeout", "5s", "file"])
            .arg(&ready),
        0,
    );
    assert_eq!(body["met"], true);
    assert_eq!(body["looks"], 1);

    let gone = h.path().join("never");
    let body = json(
        yuzu(h.path())
            .args(["wait", "--timeout", "1s", "--every", "1s", "file"])
            .arg(&gone),
        1,
    );
    assert_eq!(body["timed_out"], true);

    yuzu(h.path())
        .args(["wait", "--timeout", "soon", "file", "x"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("write 90s, 10m, or 1h"));
}

#[test]
fn sessions_counts_calls_and_the_hooks_decisions() {
    let h = home(Some(ENVIRONMENTS));
    // A transcript: the agent runs matcha before the user mentions it.
    let project = h.path().join("transcripts/-repo");
    std::fs::create_dir_all(&project).unwrap();
    let now = "2099-01-01T00:00:00Z";
    let lines = [
        serde_json::json!({"type": "user", "timestamp": now, "cwd": "/work/repo", "message": {"content": "how are forms done?"}}),
        serde_json::json!({"type": "assistant", "timestamp": now, "cwd": "/work/repo", "message": {"content": [
            {"type": "tool_use", "id": "a", "name": "Bash", "input": {"command": "matcha census forms --alt x"}},
            {"type": "tool_use", "id": "b", "name": "mcp__plugin_mochi-donut_matcha__drift", "input": {}}
        ]}}),
    ];
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(project.join("s1.jsonl"), text).unwrap();

    // The hook asks about a write to the work cluster, and logs it.
    let state = h.path().join("state");
    let input = r#"{"tool_name":"Bash","tool_input":{"command":"kubectl delete ns x"}}"#;
    yuzu(h.path())
        .env("XDG_STATE_HOME", &state)
        .args(["hook", "claude-code"])
        .write_stdin(input)
        .assert()
        .success();

    let mut cmd = yuzu(h.path());
    cmd.env("XDG_STATE_HOME", &state)
        .args(["sessions", "--since", "1d", "--dir"])
        .arg(h.path().join("transcripts"));
    let body = json(&mut cmd, 0);
    let matcha = &body["tools"][0];
    assert_eq!(matcha["tool"], "matcha");
    assert_eq!(matcha["calls"], 2);
    assert_eq!(matcha["unprompted_calls"], 2);
    assert_eq!(matcha["by_command"]["census"], 1);
    assert_eq!(matcha["by_command"]["mcp:drift"], 1);
    assert_eq!(body["hook"]["asks"], 1);
    assert_eq!(body["hook"]["recent"][0]["command"], "kubectl delete ns x");
}
