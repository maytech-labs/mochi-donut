//! `matcha eval` against a real git repo with labeled findings.

// Test helpers may panic; clippy's allow-unwrap-in-tests only covers #[test] fns.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::Command as Process;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{Value, json};

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
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// A repo whose first commit has two broken doc links, and whose second
/// fixes one. Returns the dir and both commits.
fn repo() -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(
        dir.path().join("a.md"),
        "# A\n\n[x](gone.md) and [y](also-gone.md)\n",
    )
    .unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "one"]);
    let first = git(dir.path(), &["rev-parse", "HEAD"]);
    std::fs::write(dir.path().join("gone.md"), "# Gone\n").unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "two"]);
    let second = git(dir.path(), &["rev-parse", "HEAD"]);
    (dir, first, second)
}

fn spec(dir: &Path, rev: &str, real: &[&str], false_: &[&str]) -> std::path::PathBuf {
    let path = dir.join("spec.json");
    let body = json!({"cases": [{
        "name": "links",
        "repo": dir.join("repo").display().to_string(),
        "rev": rev,
        "run": ["matcha", "drift", "--json"],
        "findings": "/failing",
        "key": "{doc}:{line} {claim}",
        "real": real,
        "false": false_,
        "min_precision": 0.9,
        "min_recall": 1.0,
    }]});
    std::fs::write(&path, body.to_string()).unwrap();
    path
}

#[test]
fn scores_a_pinned_commit_against_labels() {
    let (repo, first, second) = repo();
    let work = tempfile::tempdir().unwrap();
    std::fs::rename(repo.path(), work.path().join("repo")).unwrap();
    let spec_path = spec(
        work.path(),
        &first,
        &["a.md:3 gone.md", "a.md:3 also-gone.md"],
        &[],
    );

    let out = Command::cargo_bin("matcha")
        .unwrap()
        .args(["eval", "--json"])
        .arg(&spec_path)
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(body["cases"][0]["precision"], 1.0);
    assert_eq!(body["cases"][0]["recall"], 1.0);

    // At the second commit one link works: recall drops below the gate.
    let spec_path = spec(
        work.path(),
        &second,
        &["a.md:3 gone.md", "a.md:3 also-gone.md"],
        &[],
    );
    Command::cargo_bin("matcha")
        .unwrap()
        .arg("eval")
        .arg(&spec_path)
        .assert()
        .code(1)
        .stdout(predicate::str::contains("recall 50% < 100%"))
        .stdout(predicate::str::contains("missed: a.md:3 gone.md"));
}

#[test]
fn record_lists_unlabeled_findings_in_the_spec() {
    let (repo, first, _) = repo();
    let work = tempfile::tempdir().unwrap();
    std::fs::rename(repo.path(), work.path().join("repo")).unwrap();
    let spec_path = spec(work.path(), &first, &[], &["a.md:3 gone.md"]);
    Command::cargo_bin("matcha")
        .unwrap()
        .args(["eval", "--record"])
        .arg(&spec_path)
        .assert()
        .stdout(predicate::str::contains("unlabeled: a.md:3 also-gone.md"));
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&spec_path).unwrap()).unwrap();
    assert_eq!(
        saved["cases"][0]["unlabeled"],
        json!(["a.md:3 also-gone.md"])
    );
}

#[test]
fn a_bad_spec_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spec.json");
    std::fs::write(
        &path,
        r#"{"cases": [{"name": "x", "repo": ".", "run": [], "min_precison": 0.9}]}"#,
    )
    .unwrap();
    Command::cargo_bin("matcha")
        .unwrap()
        .arg("eval")
        .arg(&path)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unknown field `min_precison`"));
}
