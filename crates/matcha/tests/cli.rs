//! The matcha binary, run as an agent would run it.

// Test helpers may panic; clippy's allow-unwrap-in-tests only covers #[test] fns.
#![allow(clippy::unwrap_used)]

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

fn matcha() -> Command {
    Command::cargo_bin("matcha").unwrap()
}

#[test]
fn prints_its_version() {
    matcha()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("matcha "));
}

#[test]
fn agent_guide_says_when_to_run_what() {
    matcha()
        .arg("agent-guide")
        .assert()
        .success()
        .stdout(predicate::str::contains("When to run what"));
}

#[test]
fn unbuilt_commands_say_so_in_json_with_exit_code_3() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".git")).unwrap();

    let out = matcha()
        .args(["census", "form state", "--json", "--repo"])
        .arg(repo.path())
        .assert()
        .code(3)
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(body["status"], "not_implemented");
    assert_eq!(body["tool"], "matcha");
    assert_eq!(body["command"], "census");
}

#[test]
fn outside_a_repo_it_says_how_to_fix_it() {
    let dir = tempfile::tempdir().unwrap();
    matcha()
        .args(["drift", "--repo"])
        .arg(dir.path())
        .assert()
        .code(2)
        .stderr(predicate::str::contains("pass --repo <path>"));
}
