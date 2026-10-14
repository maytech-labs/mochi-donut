//! taro against real git repos, and a stub `gh` for the forge. cleanup's
//! fixtures merge branches every way a forge does (merge commit, rebase,
//! squash) and check it proves each and keeps the rest (AGENTS.md, Testing).
#![cfg(unix)]
// Test helpers may panic; clippy's allow-unwrap-in-tests only covers #[test] fns.
#![allow(clippy::unwrap_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command as Process;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{Value, json};

struct World {
    dir: tempfile::TempDir,
}

impl World {
    fn new() -> World {
        let w = World {
            dir: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(w.bin()).unwrap();
        std::fs::create_dir_all(w.repo()).unwrap();
        w.git(&["init", "-q", "-b", "main"]);
        w.commit("init", &[("README.md", "hi\n")]);
        w
    }

    fn repo(&self) -> PathBuf {
        self.dir.path().join("repo")
    }

    fn bin(&self) -> PathBuf {
        self.dir.path().join("bin")
    }

    fn git(&self, args: &[&str]) -> String {
        self.git_as("T", "t@t", args)
    }

    fn git_as(&self, name: &str, email: &str, args: &[&str]) -> String {
        let out = Process::new("git")
            .arg("-C")
            .arg(self.repo())
            .args([
                "-c",
                &format!("user.name={name}"),
                "-c",
                &format!("user.email={email}"),
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn commit(&self, msg: &str, files: &[(&str, &str)]) {
        for (p, t) in files {
            let full = self.repo().join(p);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, t).unwrap();
        }
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    fn stub_gh(&self, body: &str) {
        let p = self.bin().join("gh");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn taro(&self) -> Command {
        let mut cmd = Command::cargo_bin("taro").unwrap();
        cmd.current_dir(self.repo())
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin().display()));
        cmd
    }

    fn json(&self, args: &[&str], code: i32) -> Value {
        let out = self
            .taro()
            .args(args)
            .arg("--json")
            .assert()
            .code(code)
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice(&out)
            .unwrap_or_else(|e| panic!("not JSON ({e}): {}", String::from_utf8_lossy(&out)))
    }
}

/// Branches merged into main by a merge commit, a rebase, and a squash, one
/// left open, and one squashed but changed again after.
fn merged_world() -> World {
    let w = World::new();
    let branch = |name: &str, commits: &[(&str, &str, &str)]| {
        w.git(&["checkout", "-q", "-b", name, "main"]);
        for (msg, file, text) in commits {
            w.commit(msg, &[(file, text)]);
        }
        w.git(&["checkout", "-q", "main"]);
    };
    branch("by-merge", &[("feat: a", "a.txt", "a\n")]);
    w.git(&[
        "-c",
        "user.name=T",
        "-c",
        "user.email=t@t",
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "merge",
        "by-merge",
    ]);

    branch(
        "by-rebase",
        &[
            ("feat: b1", "b.txt", "b1\n"),
            ("feat: b2", "b2.txt", "b2\n"),
        ],
    );
    // main moves on first, as it does before a real rebase merge.
    w.commit("chore: main moves", &[("main.txt", "m\n")]);
    for c in w.git(&["rev-list", "--reverse", "main..by-rebase"]).lines() {
        w.git(&["cherry-pick", c]);
    }

    branch(
        "by-squash",
        &[("feat: c1", "c.txt", "c1\n"), ("feat: c2", "c.txt", "c2\n")],
    );
    w.git(&["merge", "-q", "--squash", "by-squash"]);
    w.git(&["commit", "-q", "-m", "feat: c (squashed)"]);

    branch("open", &[("feat: d", "d.txt", "d\n")]);

    branch(
        "half",
        &[
            ("feat: e1", "e1.txt", "e1\n"),
            ("feat: e2", "e2.txt", "e2\n"),
        ],
    );
    let first = w
        .git(&["rev-list", "--reverse", "main..half"])
        .lines()
        .next()
        .unwrap()
        .to_owned();
    w.git(&["cherry-pick", &first]);
    w
}

#[test]
fn cleanup_proves_every_merge_method_and_keeps_the_rest() {
    let w = merged_world();
    let body = w.json(&["cleanup", "--base", "main"], 0);
    let by_name = |n: &str| {
        body["branches"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["name"] == n)
            .unwrap()
            .clone()
    };
    assert_eq!(by_name("by-merge")["proof"], "ancestor");
    assert_eq!(by_name("by-rebase")["proof"], "every_patch");
    assert_eq!(by_name("by-squash")["proof"], "squashed");
    assert!(by_name("open")["proof"].is_null());
    assert!(
        by_name("open")["unsafe_because"]
            .as_str()
            .unwrap()
            .contains("1 commit not in main, such as \"feat: d\"")
    );
    assert!(
        by_name("half")["proof"].is_null(),
        "half landed is not merged"
    );
}

#[test]
fn cleanup_keeps_a_branch_checked_out_in_a_worktree() {
    let w = merged_world();
    // A new branch with no commits yet is "merged", but someone's on it.
    w.git(&["branch", "started", "main"]);
    let wt = w.dir.path().join("wt");
    w.git(&["worktree", "add", "-q", wt.to_str().unwrap(), "started"]);
    let body = w.json(&["cleanup", "--base", "main"], 0);
    let started = body["branches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["name"] == "started")
        .unwrap()
        .clone();
    assert!(started["proof"].is_null(), "{started}");
    assert!(
        started["unsafe_because"]
            .as_str()
            .unwrap()
            .contains("checked out in the worktree at")
    );
    let body = w.json(&["cleanup", "--base", "main", "--delete", "--yes"], 0);
    assert!(
        body["deleted"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d != "started")
    );
    assert!(w.git(&["branch", "--list", "started"]).contains("started"));
}

#[test]
fn cleanup_deletes_only_proved_branches_and_only_when_told() {
    let w = merged_world();
    // No terminal to ask on, and no --yes: refuse.
    w.taro()
        .args(["cleanup", "--base", "main", "--delete"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("pass --yes"));
    assert!(
        w.git(&["branch", "--list", "by-merge"])
            .contains("by-merge")
    );

    let body = w.json(&["cleanup", "--base", "main", "--delete", "--yes"], 0);
    let mut deleted: Vec<String> = body["deleted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d.as_str().unwrap().to_owned())
        .collect();
    deleted.sort();
    assert_eq!(deleted, ["by-merge", "by-rebase", "by-squash"]);
    let left = w.git(&["branch", "--format=%(refname:short)"]);
    assert!(left.contains("open") && left.contains("half"), "{left}");
}

#[test]
fn owners_reads_codeowners_and_history() {
    let w = World::new();
    w.commit(
        "owners",
        &[(".github/CODEOWNERS", "* @team\n/crates/yuzu/ @yuzu-owner\n")],
    );
    w.git(&["checkout", "-q", "-b", "work"]);
    w.git_as(
        "Ada",
        "ada@x",
        &["commit", "-q", "--allow-empty", "-m", "x"],
    );
    std::fs::create_dir_all(w.repo().join("crates/yuzu")).unwrap();
    std::fs::write(w.repo().join("crates/yuzu/a.rs"), "a\n").unwrap();
    w.git(&["add", "-A"]);
    w.git_as("Ada", "ada@x", &["commit", "-q", "-m", "yuzu a"]);
    w.git_as(
        "Ada",
        "ada@x",
        &["commit", "-q", "--allow-empty", "-m", "y"],
    );
    std::fs::write(w.repo().join("crates/yuzu/a.rs"), "b\n").unwrap();
    std::fs::write(w.repo().join("other.txt"), "o\n").unwrap();
    w.git(&["add", "-A"]);
    w.git_as("Bo", "bo@x", &["commit", "-q", "-m", "yuzu b"]);

    let body = w.json(&["owners", "--diff", "--base", "main"], 0);
    let paths: Vec<(String, String)> = body["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["path"].as_str().unwrap().to_owned(),
                p["owners"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|o| o.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        })
        .collect();
    assert_eq!(
        paths,
        [
            ("crates/yuzu/a.rs".to_owned(), "@yuzu-owner".to_owned()),
            ("other.txt".to_owned(), "@team".to_owned())
        ]
    );
    let suggested = body["suggested"].as_array().unwrap();
    assert_eq!(suggested[0], "@yuzu-owner");
    assert!(suggested.iter().any(|s| s == "Ada <ada@x>"), "{body}");
}

/// A gh stand-in for PR 7: a failing check, requested changes, one open
/// thread, ten merged PRs that all touch the changelog, and no readable
/// branch rules.
const GH_BLOCKED: &str = r#"case "$*" in
  "pr view 7 "*) echo '{"number":7,"title":"Add thing","url":"u","state":"OPEN","isDraft":false,"mergeable":"MERGEABLE","mergeStateStatus":"BLOCKED","reviewDecision":"CHANGES_REQUESTED","headRefName":"feat/x","baseRefName":"main","files":[{"path":"src/a.rs"}],"statusCheckRollup":[{"__typename":"CheckRun","name":"linux","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"https://ci/1"}]}';;
  *"rules/branches"*|*"protection"*) echo 'gh: Upgrade to GitHub Pro (HTTP 403)' >&2; exit 1;;
  *graphql*) echo '{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"isResolved":false,"isOutdated":true,"path":"src/a.rs","line":4,"comments":{"nodes":[{"author":{"login":"rev"},"body":"Use the clock.","url":"https://pr/7#t"}]},"first":{"nodes":[{"author":{"login":"rev"}}]}}]}}}}}';;
  "pr list "*) printf '['; for i in 1 2 3 4 5 6 7 8 9 10; do [ $i -gt 1 ] && printf ','; printf '{"title":"feat: x%s","files":[{"path":"CHANGELOG.md"}]}' $i; done; echo ']';;
  *) echo "unexpected: $*" >&2; exit 1;;
esac"#;

#[test]
fn ready_lists_each_blocker_and_learned_convention() {
    let w = World::new();
    w.stub_gh(GH_BLOCKED);
    let body = w.json(&["ready", "7", "--repo", "acme/app"], 1);
    let state = |n: &str| {
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["name"] == n)
            .unwrap()["state"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(state("checks"), "block");
    assert_eq!(state("reviews"), "block");
    assert_eq!(state("threads"), "block");
    assert_eq!(state("changelog"), "block");
    assert_eq!(state("title"), "block");
    assert_eq!(state("conflicts"), "ok");
    w.taro()
        .args(["ready", "7", "--repo", "acme/app"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Failed: linux (https://ci/1)."))
        .stdout(predicate::str::contains("every check counts"))
        .stdout(predicate::str::contains(
            "10 of the last 10 merged PRs changed a changelog; this one doesn't.",
        ));
}

/// A gh stand-in for PR 8, every check reported, with `graphql` and
/// `rules` answering as given and classic protection unreadable (403, as
/// for anyone without admin rights).
fn gh_pr8(merge_state: &str, graphql: &str, rules: &str) -> String {
    format!(
        r#"case "$*" in
  "pr view 8 "*) echo '{{"number":8,"title":"fix: y","url":"u","state":"OPEN","isDraft":false,"mergeable":"MERGEABLE","mergeStateStatus":"{merge_state}","reviewDecision":"","headRefName":"fix/y","baseRefName":"main","files":[{{"path":"src/a.rs"}}],"statusCheckRollup":[{{"__typename":"CheckRun","name":"linux","status":"COMPLETED","conclusion":"SUCCESS"}},{{"__typename":"CheckRun","name":"docs","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"https://ci/9"}}]}}';;
  *graphql*) echo '{graphql}';;
  *"rules/branches"*) echo '{rules}';;
  *"protection"*) echo 'gh: Must have admin rights to Repository. (HTTP 403)' >&2; exit 1;;
  "pr list "*) echo '[]';;
  *) echo "unexpected: $*" >&2; exit 1;;
esac"#
    )
}

#[test]
fn ready_judges_checks_by_what_github_says_is_required() {
    let w = World::new();
    let threads = r#""reviewThreads":{"nodes":[]}"#;
    // GitHub says only linux is required: the failing docs check warns.
    let graphql = format!(
        r#"{{"data":{{"repository":{{"pullRequest":{{{threads},"commits":{{"nodes":[{{"commit":{{"statusCheckRollup":{{"contexts":{{"nodes":[{{"__typename":"CheckRun","name":"linux","isRequired":true}},{{"__typename":"CheckRun","name":"docs","isRequired":false}}]}}}}}}}}]}}}}}}}}}}"#
    );
    w.stub_gh(&gh_pr8("UNSTABLE", &graphql, "[]"));
    let body = w.json(&["ready", "8", "--repo", "acme/app"], 0);
    let checks = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "checks")
        .unwrap()
        .clone();
    assert_eq!(checks["state"], "warn", "{body}");
    assert_eq!(body["ready"], true);

    // No isRequired, no rulesets, and protection unreadable: that isn't
    // "nothing required", so the failing check blocks.
    let graphql = format!(r#"{{"data":{{"repository":{{"pullRequest":{{{threads}}}}}}}}}"#);
    w.stub_gh(&gh_pr8("BLOCKED", &graphql, "[]"));
    w.taro()
        .args(["ready", "8", "--repo", "acme/app"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Failed: docs (https://ci/9)."))
        .stdout(predicate::str::contains("every check counts"));
}

#[test]
fn ready_counts_every_check_when_the_rules_require_none() {
    let w = World::new();
    // GitHub answers isRequired: false for both, as for a branch with no
    // rules. That says nothing is required, not that CI doesn't matter.
    let graphql = r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[]},"commits":{"nodes":[{"commit":{"statusCheckRollup":{"contexts":{"nodes":[{"__typename":"CheckRun","name":"linux","isRequired":false},{"__typename":"CheckRun","name":"docs","isRequired":false}]}}}}]}}}}}"#;
    w.stub_gh(&gh_pr8("UNSTABLE", graphql, "[]"));
    w.taro()
        .args(["ready", "8", "--repo", "acme/app"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Failed: docs"))
        .stdout(predicate::str::contains(
            "require no check, so every check counts",
        ));
}

#[test]
fn ready_blocks_when_github_does_for_a_reason_taro_cant_see() {
    let w = World::new();
    let graphql = r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[]},"commits":{"nodes":[{"commit":{"statusCheckRollup":{"contexts":{"nodes":[{"__typename":"CheckRun","name":"linux","isRequired":true},{"__typename":"CheckRun","name":"docs","isRequired":false}]}}}}]}}}}}"#;
    // Signed commits, say: everything taro reads is fine, GitHub still says no.
    w.stub_gh(&gh_pr8("BLOCKED", graphql, "[]"));
    w.taro()
        .args(["ready", "8", "--repo", "acme/app"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("for a reason taro doesn't read"))
        .stdout(predicate::str::contains("gh pr view 8 --web"));
    w.stub_gh(&gh_pr8("BEHIND", graphql, "[]"));
    w.taro()
        .args(["ready", "8", "--repo", "acme/app"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("gh pr update-branch 8"));
}

#[test]
fn reviews_lists_open_threads() {
    let w = World::new();
    w.stub_gh(GH_BLOCKED);
    w.taro()
        .args(["reviews", "7", "--repo", "acme/app"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "[ ]  src/a.rs:4  from rev  (code changed since)",
        ))
        .stdout(predicate::str::contains("rev: Use the clock."))
        .stdout(predicate::str::contains(
            "1 unresolved review thread on PR #7.",
        ));
}

#[test]
fn flaky_finds_reruns_and_mixed_outcomes() {
    let w = World::new();
    w.stub_gh(
        r#"case "$*" in
  *"actions/runs?"*) echo '{"workflow_runs":[
    {"id":1,"name":"CI","head_sha":"a","conclusion":"success","run_attempt":2,"created_at":"2099-01-02T00:00:00Z","html_url":"https://ci/1"},
    {"id":2,"name":"CI","head_sha":"b","conclusion":"failure","run_attempt":1,"created_at":"2099-01-03T00:00:00Z","html_url":"https://ci/2"},
    {"id":3,"name":"CI","head_sha":"c","conclusion":"failure","run_attempt":1,"created_at":"2099-01-04T00:00:00Z","html_url":"https://ci/3"}]}';;
  *"runs/1/jobs"*) echo '{"jobs":[{"name":"check (macos-15)","conclusion":"success"},{"name":"check (ubuntu)","conclusion":"success"}]}';;
  *"runs/1/attempts/1/jobs"*) echo '{"jobs":[{"name":"check (macos-15)","conclusion":"failure"},{"name":"check (ubuntu)","conclusion":"success"}]}';;
  *) echo "unexpected: $*" >&2; exit 1;;
esac"#,
    );
    let body = w.json(&["flaky", "--repo", "acme/app"], 0);
    let checks: Vec<&str> = body["flaky"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["check"].as_str().unwrap())
        .collect();
    assert_eq!(checks, ["CI / check (macos-15)"], "{body}");
    assert_eq!(body["flaky"][0]["runs"], 3);
}

#[test]
fn flaky_ignores_reruns_of_green_runs_and_cancels() {
    let w = World::new();
    w.stub_gh(
        r#"case "$*" in
  *"actions/runs?"*) echo '{"workflow_runs":[
    {"id":1,"name":"CI","head_sha":"a","conclusion":"success","run_attempt":2,"created_at":"2099-01-02T00:00:00Z","html_url":"https://ci/1"},
    {"id":2,"name":"CI","head_sha":"b","conclusion":"cancelled","run_attempt":1,"created_at":"2099-01-03T00:00:00Z","html_url":"https://ci/2"},
    {"id":3,"name":"CI","head_sha":"b","conclusion":"success","run_attempt":1,"created_at":"2099-01-04T00:00:00Z","html_url":"https://ci/3"}]}';;
  *"runs/1/jobs"*) echo '{"jobs":[{"name":"check","conclusion":"success"}]}';;
  *"runs/1/attempts/1/jobs"*) echo '{"jobs":[{"name":"check","conclusion":"success"}]}';;
  *) echo "unexpected: $*" >&2; exit 1;;
esac"#,
    );
    let body = w.json(&["flaky", "--repo", "acme/app"], 0);
    assert_eq!(body["flaky"], json!([]), "{body}");
}

#[test]
fn forge_errors_say_what_to_do() {
    let w = World::new();
    w.stub_gh("echo 'HTTP 401: Bad credentials (https://api.github.com/graphql)' >&2; exit 1");
    w.taro()
        .args(["ready", "7", "--repo", "acme/app"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("yuzu whoami --verify"));
    // A bad GH_TOKEN is named, and isn't mistaken for a missing PR.
    w.git(&["checkout", "-q", "-b", "feat/x"]);
    w.taro()
        .args(["ready", "--repo", "acme/app"])
        .env("GH_TOKEN", "bad")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("GH_TOKEN is set"))
        .stderr(predicate::str::contains("No open PR").not());
    w.taro()
        .args(["ready", "7", "--repo", "gitlab.com/a/b"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "GitLab, which taro doesn't support yet",
        ));
}

#[test]
fn mcp_serves_ready_reviews_owners_and_flaky() {
    let w = World::new();
    let input = format!(
        "{}\n",
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})
    );
    let out = w
        .taro()
        .arg("mcp")
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let reply: Value = serde_json::from_slice(&out).unwrap();
    let names: Vec<&str> = reply["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["ready", "reviews", "owners", "flaky"]);
}
