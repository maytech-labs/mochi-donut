//! drift against real git repos built in a temp directory.

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

/// A repo whose docs make claims about its code, most of them true.
fn repo(conventions: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let files: &[(&str, &str)] = &[
        (
            "backend/src/money.rs",
            "pub struct Cents(pub i64);\npub fn round_half_even() {}\n",
        ),
        (
            "backend/src/clock.rs",
            "pub trait Clock { fn today(&self) -> NaiveDate; }\nfn now() { Utc::now(); }\n",
        ),
        ("frontend/src/lib/money.ts", "export const cents = 1;\n"),
        (
            "frontend/package.json",
            r#"{"scripts": {"test": "vitest", "lint": "eslint ."}}"#,
        ),
        ("Makefile", "openapi:\n\techo\ncheck: lint test\n\techo\n"),
        (
            "docs/glossary.md",
            "# Glossary\n\n## Policy term\n\nA term.\n",
        ),
        ("docs/engineering/conventions.md", conventions),
    ];
    for (path, text) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    dir
}

const CONVENTIONS: &str = "\
# Conventions

- Use the [glossary](../glossary.md#policy-term) names.
- Use newtypes for IDs (`ClientId(Uuid)`) and money (`Cents(i64)`).
- Domain code takes a `Clock` and calls `Clock::today()`, rather than `Utc::now()`.
- Only `lib/money.ts` formats cents. Run `make openapi` and `npm run lint`.

```rust
let id = OldThing::new();
```
";

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
fn flags_a_type_the_code_no_longer_has() {
    let repo = repo(CONVENTIONS);
    matcha(repo.path())
        .arg("drift")
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "docs/engineering/conventions.md:4",
        ))
        .stdout(predicate::str::contains("`ClientId(Uuid)`"))
        .stdout(predicate::str::contains(
            "FAILS: `ClientId` appears in no source file",
        ))
        .stdout(predicate::str::contains("1 fail"));
}

#[test]
fn json_lists_failures_with_counts() {
    let repo = repo(CONVENTIONS);
    let body = json(matcha(repo.path()).arg("drift"), 1);
    assert_eq!(body["docs_checked"], 2);
    let failing = body["failing"].as_array().unwrap();
    assert_eq!(failing.len(), 1, "{body}");
    assert_eq!(failing[0]["doc"], "docs/engineering/conventions.md");
    assert_eq!(failing[0]["line"], 4);
    assert_eq!(failing[0]["kind"], "symbol");
    // The glossary link and its anchor, Cents, Clock, Clock::today,
    // lib/money.ts, make openapi, npm run lint. Utc::now is in a "rather
    // than" clause, and code blocks are examples, not claims.
    assert_eq!(body["holds"], 7, "{body}");
}

#[test]
fn true_docs_pass() {
    let repo =
        repo("# Conventions\n\nMoney is `Cents(i64)`, see [money](../../backend/src/money.rs).\n");
    matcha(repo.path())
        .arg("drift")
        .assert()
        .code(0)
        .stdout(predicate::str::contains("0 fail"));
}

#[test]
fn broken_links_and_anchors_fail_with_a_fix() {
    let repo = repo(
        "# Conventions\n\n\
         See [the glossary](../glossary.md#policy-period) and [money](money.rs).\n",
    );
    let body = json(matcha(repo.path()).arg("drift"), 1);
    let failing = body["failing"].as_array().unwrap();
    assert_eq!(failing.len(), 2, "{body}");
    assert_eq!(failing[0]["kind"], "anchor");
    assert!(
        failing[0]["fix"].as_str().unwrap().contains("#policy-term"),
        "{body}"
    );
    assert_eq!(failing[1]["kind"], "link");
    assert!(
        failing[1]["fix"]
            .as_str()
            .unwrap()
            .contains("backend/src/money.rs"),
        "{body}"
    );
}

#[test]
fn missing_make_targets_and_scripts_fail() {
    let repo = repo("# Conventions\n\nRun `make deploy` and `npm run e2e`.\n");
    let body = json(matcha(repo.path()).arg("drift"), 1);
    let failing = body["failing"].as_array().unwrap();
    assert_eq!(failing.len(), 2, "{body}");
    assert_eq!(failing[0]["kind"], "make_target");
    assert_eq!(failing[1]["kind"], "npm_script");
}

#[test]
fn a_skip_comment_silences_the_next_line() {
    let repo = repo("# Conventions\n\n<!-- matcha: skip -->\nAnother repo has `ClientId(Uuid)`.\n");
    matcha(repo.path()).arg("drift").assert().code(0);
}

#[test]
fn in_limits_which_docs_are_checked() {
    let repo = repo(CONVENTIONS);
    let body = json(
        matcha(repo.path()).args(["drift", "--in", "docs/glossary.md"]),
        0,
    );
    assert_eq!(body["docs_checked"], 1);
}

#[test]
fn ignored_paths_are_skipped_not_failed() {
    let repo = repo("# Setup\n\nRun `make openapi`, fill in `.env`, and look in `target/`.\n");
    std::fs::write(repo.path().join(".gitignore"), ".env\ntarget/\n").unwrap();
    git(repo.path(), &["add", "-A"]);
    git(repo.path(), &["commit", "-q", "-m", "env"]);
    let body = json(matcha(repo.path()).arg("drift"), 0);
    assert_eq!(body["holds"], 1, "{body}");
    assert_eq!(body["skipped"], 2, "{body}");
}

#[test]
fn check_comments_prove_or_refute_their_sentence() {
    // clock.rs calls Utc::now(); nothing else does.
    let holds = repo(
        "# Time\n\n<!-- matcha: no-match 'Utc::now\\(' except 'clock.rs' -->\nNever call `Utc::now()` outside a `Clock`.\n",
    );
    let body = json(matcha(holds.path()).arg("drift"), 0);
    // The rule holds. `Utc::now()` and `Clock` sit in a "never" clause, so
    // they aren't symbol claims.
    assert_eq!(body["holds"], 1, "{body}");

    let fails =
        repo("# Time\n\n<!-- matcha: no-match 'Utc::now\\(' -->\nNever call `Utc::now()`.\n");
    matcha(fails.path())
        .arg("drift")
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "docs/engineering/conventions.md:4",
        ))
        .stdout(predicate::str::contains("\"Never call `Utc::now()`.\""))
        .stdout(predicate::str::contains(
            "found 1 line in 1 file (backend/src/clock.rs:2)",
        ));
}

#[test]
fn a_check_comment_that_cant_run_fails() {
    let repo = repo("# Time\n\n<!-- matcha: forbid 'x' -->\nNo x.\n");
    let body = json(matcha(repo.path()).arg("drift"), 1);
    assert_eq!(body["failing"][0]["kind"], "rule");
    assert!(
        body["failing"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("unknown check"),
        "{body}"
    );
}

#[test]
fn suggest_proposes_comments_with_evidence() {
    let repo = repo("# Time\n\nNever call `Utc::now()` outside `clock.rs`.\n");
    let body = json(matcha(repo.path()).args(["drift", "--suggest"]), 0);
    let s = &body["suggestions"][0];
    assert_eq!(s["line"], 3, "{body}");
    assert_eq!(
        s["comment"],
        "<!-- matcha: no-match 'Utc::now\\(\\)' except 'clock.rs' -->"
    );
    assert_eq!(s["holds_now"], true);
}
