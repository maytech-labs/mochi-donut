//! ledger and its hook against real git repos built in a temp directory.

// Test helpers may panic; clippy's allow-unwrap-in-tests only covers #[test] fns.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::Command as Process;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Process::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?} failed");
    String::from_utf8(out.stdout).unwrap()
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "feat/fein-tests"]);
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/test-data.md"), "# Test data\n").unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    dir
}

fn matcha(repo: &Path) -> Command {
    let mut cmd = Command::cargo_bin("matcha").unwrap();
    cmd.arg("--repo").arg(repo);
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

#[test]
fn records_claims_per_branch_outside_git() {
    let repo = repo();
    let body = json(
        matcha(repo.path()).args([
            "ledger",
            "add",
            "assumed",
            "FEINs in tests use a 00- prefix",
        ]),
        0,
    );
    assert_eq!(body["entry"]["id"], 1);
    assert_eq!(body["entry"]["status"], "assumed");
    assert!(
        repo.path()
            .join(".matcha/ledger/feat%2Ffein-tests.jsonl")
            .exists()
    );
    // The ledger ignores itself, so the repo stays clean.
    assert_eq!(git(repo.path(), &["status", "--porcelain"]), "");

    let list = json(matcha(repo.path()).args(["ledger", "list"]), 0);
    assert_eq!(list["branch"], "feat/fein-tests");
    assert_eq!(
        list["entries"][0]["claim"],
        "FEINs in tests use a 00- prefix"
    );
}

#[test]
fn verified_needs_evidence() {
    let repo = repo();
    matcha(repo.path())
        .args([
            "ledger",
            "add",
            "verified",
            "Traefik listens on all three nodes",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("needs evidence"));
    let body = json(
        matcha(repo.path()).args([
            "ledger",
            "add",
            "verified",
            "The test data doc has a heading",
            "--file",
            "docs/test-data.md:1",
        ]),
        0,
    );
    assert_eq!(body["entry"]["evidence"]["file"], "docs/test-data.md");
    assert_eq!(body["entry"]["evidence"]["output"], "# Test data");
}

#[test]
fn check_diff_flags_assumed_claims_until_verified() {
    let repo = repo();
    matcha(repo.path())
        .args([
            "ledger",
            "add",
            "assumed",
            "FEINs in tests use a 00- prefix",
        ])
        .assert()
        .success();
    std::fs::write(
        repo.path().join("docs/test-data.md"),
        "# Test data\n\nFEINs use the 00- prefix.\n",
    )
    .unwrap();

    matcha(repo.path())
        .args(["ledger", "check", "--diff"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "docs/test-data.md:3 states an assumed claim:",
        ))
        .stdout(predicate::str::contains(
            "(assumed, no evidence; 3 of 4 words)",
        ));

    matcha(repo.path())
        .args([
            "ledger",
            "verify",
            "1",
            "--evidence",
            "grep -r '00-' tests: 41 of 41",
        ])
        .assert()
        .success();
    let body = json(matcha(repo.path()).args(["ledger", "check", "--diff"]), 0);
    assert_eq!(body["flagged"], serde_json::json!([]));
}

#[test]
fn the_hook_records_commands_as_evidence() {
    let repo = repo();
    let input = serde_json::json!({
        "session_id": "s",
        "cwd": repo.path(),
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": "kubectl get svc -A"},
        "tool_response": {"stdout": "traefik LoadBalancer 10.0.0.127,10.0.0.24,10.0.0.8\nAPI_TOKEN=abc123", "stderr": ""}
    });
    Command::cargo_bin("matcha")
        .unwrap()
        .args(["hook", "claude-code"])
        .write_stdin(input.to_string())
        .assert()
        .success()
        .stdout("");

    let body = json(
        matcha(repo.path()).args([
            "ledger",
            "add",
            "verified",
            "Traefik listens on all three nodes",
            "--cmd",
            "get svc",
        ]),
        0,
    );
    let ev = &body["entry"]["evidence"];
    assert_eq!(ev["command"], "kubectl get svc -A");
    assert_eq!(
        ev["output"],
        "traefik LoadBalancer 10.0.0.127,10.0.0.24,10.0.0.8\nAPI_TOKEN=[redacted]"
    );

    matcha(repo.path())
        .args(["ledger", "summary"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "Verified:\n- Traefik listens on all three nodes (`kubectl get svc -A`: traefik LoadBalancer",
        ));
}

#[test]
fn the_hook_ignores_bad_input_and_other_tools() {
    for input in ["not json", r#"{"tool_name": "Read", "cwd": "/"}"#] {
        Command::cargo_bin("matcha")
            .unwrap()
            .args(["hook", "claude-code"])
            .write_stdin(input)
            .assert()
            .success()
            .stdout("");
    }
}

#[test]
fn cmd_evidence_without_the_hook_explains_itself() {
    let repo = repo();
    matcha(repo.path())
        .args(["ledger", "add", "verified", "x y z", "--cmd", "kubectl"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "Commands are recorded by the hook",
        ));
}

#[test]
fn archive_moves_the_ledger_aside() {
    let repo = repo();
    matcha(repo.path())
        .args(["ledger", "add", "assumed", "a b c"])
        .assert()
        .success();
    let body = json(matcha(repo.path()).args(["ledger", "archive"]), 0);
    let to = body["archived_to"].as_str().unwrap();
    assert!(
        to.contains(".matcha/ledger/archive/feat%2Ffein-tests-"),
        "{to}"
    );
    assert!(Path::new(to).exists());
    let list = json(matcha(repo.path()).args(["ledger", "list"]), 0);
    assert_eq!(list["entries"], serde_json::json!([]));
}

#[test]
fn a_corrupt_ledger_is_an_error_not_an_empty_one() {
    let repo = repo();
    matcha(repo.path())
        .args(["ledger", "add", "assumed", "a b c"])
        .assert()
        .success();
    let path = repo.path().join(".matcha/ledger/feat%2Ffein-tests.jsonl");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("{oops\n");
    std::fs::write(&path, text).unwrap();
    matcha(repo.path())
        .args(["ledger", "list"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(":2 isn't a valid ledger line"));
}

#[test]
fn check_catches_paraphrases_but_not_paragraphs_on_the_topic() {
    let repo = repo();
    matcha(repo.path())
        .args([
            "ledger",
            "add",
            "assumed",
            "Chrome's Local Network permission resets on every update",
        ])
        .assert()
        .success();
    std::fs::write(
        repo.path().join("docs/test-data.md"),
        "# Notes\n\nUpdates to Chrome can reset the permission.\n\nThe probe's facts give exactly the Local Network permission, with Chrome's update named, from the session on that day. It's a unit test.\n",
    )
    .unwrap();
    let body = json(matcha(repo.path()).args(["ledger", "check", "--diff"]), 1);
    let flagged = body["flagged"].as_array().unwrap();
    assert_eq!(flagged.len(), 1, "{body}");
    assert_eq!(flagged[0]["line"], 3);
    assert_eq!(
        flagged[0]["text"],
        "Updates to Chrome can reset the permission."
    );
}
