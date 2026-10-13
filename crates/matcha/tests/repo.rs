//! census and recipe against real git repos built in a temp directory.

// Test helpers may panic; clippy's allow-unwrap-in-tests only covers #[test] fns.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::Command as Process;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

fn git(dir: &Path, args: &[&str]) {
    let ok = Process::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?} failed");
}

/// Writes `files` (path, contents) and commits them with `subject`.
fn commit(dir: &Path, subject: &str, files: &[(&str, &str)]) {
    for (path, text) in files {
        let full = dir.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", subject]);
}

/// A repo where every change to the API spec source also regenerated the
/// committed spec, six times over, among unrelated work.
fn api_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(
        dir.path().join("Makefile"),
        "openapi:\n\tcargo run -- openapi > web/openapi.json\n",
    )
    .unwrap();
    // Unrelated work, so the spec isn't simply in every commit.
    for i in 0..5 {
        commit(
            dir.path(),
            &format!("fix: tidy {i}"),
            &[(&format!("lib/util{i}.rs"), "\n")],
        );
    }
    for (i, name) in [
        "clients", "holders", "policies", "invoices", "payments", "quotes",
    ]
    .iter()
    .enumerate()
    {
        commit(
            dir.path(),
            &format!("feat: add {name} endpoint"),
            &[
                ("api/openapi.rs", &format!("// v{i}\n")),
                ("web/openapi.json", &format!("{{\"v\": {i}}}\n")),
                (&format!("api/routes/{name}.rs"), "pub fn list() {}\n"),
            ],
        );
    }
    commit(
        dir.path(),
        "docs: cancelled, not canceled",
        &[("README.md", "A cancelled term. isCancelled.\n")],
    );
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
fn recipe_check_names_the_missing_companion_and_the_fix() {
    let repo = api_repo();
    std::fs::write(repo.path().join("api/openapi.rs"), "// changed\n").unwrap();

    matcha(repo.path())
        .args(["recipe", "--check"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "api/openapi.rs changed but web/openapi.json didn't.",
        ))
        .stdout(predicate::str::contains("6 of 6 earlier changes"))
        .stdout(predicate::str::contains("Usual fix: make openapi"));
}

#[test]
fn recipe_check_passes_when_the_companion_changed_too() {
    let repo = api_repo();
    std::fs::write(repo.path().join("api/openapi.rs"), "// changed\n").unwrap();
    std::fs::write(repo.path().join("web/openapi.json"), "{}\n").unwrap();

    let body = json(matcha(repo.path()).args(["recipe", "--check"]), 0);
    assert_eq!(body["missing"], serde_json::json!([]));
    assert_eq!(body["commits_learned_from"], 12);
}

#[test]
fn recipe_check_can_judge_a_past_commit() {
    let repo = api_repo();
    commit(
        repo.path(),
        "feat: forgot the spec",
        &[("api/openapi.rs", "// oops\n")],
    );

    let body = json(
        matcha(repo.path()).args(["recipe", "--check", "--commit", "HEAD"]),
        1,
    );
    assert_eq!(body["missing"][0]["missing"], "web/openapi.json");
    assert_eq!(body["missing"][0]["fix"], "make openapi");
}

#[test]
fn recipe_like_finds_what_similar_changes_shared() {
    let repo = api_repo();
    let body = json(
        matcha(repo.path()).args(["recipe", "--like", "add a refunds endpoint"]),
        0,
    );
    assert_eq!(body["similar"].as_array().unwrap().len(), 6);
    let paths: Vec<&str> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"api/openapi.rs"));
    assert!(paths.contains(&"api/routes/*.rs"));
}

#[test]
fn recipe_replay_measures_itself() {
    let repo = api_repo();
    commit(
        repo.path(),
        "feat: add refunds endpoint",
        &[
            ("api/openapi.rs", "// v9\n"),
            ("web/openapi.json", "{}\n"),
            ("api/routes/refunds.rs", "\n"),
        ],
    );
    let body = json(matcha(repo.path()).args(["recipe", "--replay", "1"]), 0);
    assert_eq!(body["commits_replayed"], 1);
    assert_eq!(body["caught"], 2);
    assert_eq!(body["false_alarms"], 0);
}

#[test]
fn census_counts_alternatives() {
    let repo = api_repo();
    let body = json(
        matcha(repo.path()).args([
            "census",
            "routes",
            "--alt",
            r"pub fn \w+",
            "--alt",
            "nothing_like_this",
        ]),
        0,
    );
    let alts = body["alternatives"].as_array().unwrap();
    assert_eq!(alts[0]["files"], 6);
    assert_eq!(alts[0]["by_language"]["Rust"], 6);
    assert_eq!(alts[1]["files"], 0);
}

#[test]
fn census_spelling_counts_variants() {
    let repo = api_repo();
    let body = json(
        matcha(repo.path()).args(["census", "--spelling", "cancel"]),
        0,
    );
    assert_eq!(body["variants"][0]["word"], "cancelled");
    assert_eq!(body["variants"][0]["count"], 2);
}

#[test]
fn census_without_alternatives_explains_itself() {
    let repo = api_repo();
    matcha(repo.path())
        .args(["census", "forms"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--alt <regex>"));
}

#[test]
fn recipe_check_says_when_history_is_too_thin_to_judge() {
    let repo = api_repo();
    // util0.rs changed once before: not enough to learn its companions.
    std::fs::write(repo.path().join("lib/util0.rs"), "// changed\n").unwrap();
    matcha(repo.path())
        .args(["recipe", "--check"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains(
            "Can't tell: none of the 1 changed files has 6 or more earlier changes",
        ));
    let body = json(matcha(repo.path()).args(["recipe", "--check"]), 0);
    assert_eq!(body["judged"], 0);
}
