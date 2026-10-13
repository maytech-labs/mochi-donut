//! The matcha binary, run as an agent would run it.

// Test helpers may panic; clippy's allow-unwrap-in-tests only covers #[test] fns.
#![allow(clippy::unwrap_used)]

use assert_cmd::Command;
use predicates::prelude::*;

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
fn outside_a_repo_it_says_how_to_fix_it() {
    let dir = tempfile::tempdir().unwrap();
    matcha()
        .args(["drift", "--repo"])
        .arg(dir.path())
        .assert()
        .code(2)
        .stderr(predicate::str::contains("pass --repo <path>"));
}
