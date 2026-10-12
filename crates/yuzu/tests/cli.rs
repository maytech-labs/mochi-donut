//! The yuzu binary, run as an agent or a hook would run it.

// Test helpers may panic; clippy's allow-unwrap-in-tests only covers #[test] fns.
#![allow(clippy::unwrap_used)]

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

fn yuzu() -> Command {
    Command::cargo_bin("yuzu").unwrap()
}

#[test]
fn prints_its_version() {
    yuzu()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("yuzu "));
}

#[test]
fn agent_guide_says_when_to_run_what() {
    yuzu()
        .arg("agent-guide")
        .assert()
        .success()
        .stdout(predicate::str::contains("When to run what"));
}

#[test]
fn no_subcommand_means_status() {
    let out = yuzu()
        .arg("--json")
        .assert()
        .code(3)
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(body["command"], "status");
}

#[test]
fn check_takes_the_command_after_a_double_dash() {
    yuzu()
        .args(["check", "--", "kubectl", "apply", "-f", "x.yaml"])
        .assert()
        .code(3);
    yuzu().arg("check").assert().code(2);
}
