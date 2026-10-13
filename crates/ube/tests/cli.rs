//! ube against real git repos. The checks it runs are the repo's own: stub
//! `npm` and `matcha` scripts first on PATH stand in for the tools a repo
//! would have, so ube runs unchanged and is judged by its output, exit
//! code, and what it writes (AGENTS.md, Testing).
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
    /// A repo with `files` committed, and an empty stub bin directory.
    fn new(files: &[(&str, &str)]) -> World {
        let w = World {
            dir: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(w.bin()).unwrap();
        std::fs::create_dir_all(w.repo()).unwrap();
        w.git(&["init", "-q", "-b", "main"]);
        for (p, t) in files {
            w.write(p, t);
        }
        w.git(&["add", "-A"]);
        w.git(&["commit", "-q", "-m", "init"]);
        w
    }

    fn repo(&self) -> PathBuf {
        self.dir.path().join("repo")
    }

    fn bin(&self) -> PathBuf {
        self.dir.path().join("bin")
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.repo().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Process::new("git")
            .arg("-C")
            .arg(self.repo())
            .args([
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8(out.stdout).unwrap()
    }

    fn stub(&self, name: &str, body: &str) {
        let p = self.bin().join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn ube(&self) -> Command {
        let path = format!("{}:/usr/bin:/bin", self.bin().display());
        let mut cmd = Command::cargo_bin("ube").unwrap();
        cmd.current_dir(self.repo()).env("PATH", path);
        cmd
    }

    fn json(&self, args: &[&str], code: i32) -> Value {
        let out = self
            .ube()
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

fn commands(plan: &Value) -> Vec<String> {
    plan["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["command"].as_str().unwrap().to_owned())
        .collect()
}

/// A Cargo workspace where `app` depends on `core`, with a Makefile and CI.
const WORKSPACE: &[(&str, &str)] = &[
    ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
    ("crates/core/Cargo.toml", "[package]\nname = \"core\"\n"),
    ("crates/core/src/lib.rs", "pub fn a() {}\n"),
    (
        "crates/app/Cargo.toml",
        "[package]\nname = \"app\"\n\n[dependencies]\ncore = { path = \"../core\" }\n",
    ),
    ("crates/app/src/main.rs", "fn main() {}\n"),
    ("Makefile", "check:\n\tcargo test --workspace\n"),
    (
        ".github/workflows/ci.yml",
        "jobs:\n  ci:\n    steps:\n      - uses: actions/checkout@v4\n      - run: cargo test --workspace\n",
    ),
];

#[test]
fn plan_tests_a_package_and_its_dependents() {
    let w = World::new(WORKSPACE);
    w.write("crates/core/src/lib.rs", "pub fn a() { let _ = 1; }\n");
    let plan = w.json(&["plan"], 0);
    assert_eq!(commands(&plan), ["cargo test -p core", "cargo test -p app"]);
    assert!(
        plan["checks"][1]["reason"]
            .as_str()
            .unwrap()
            .contains("app depends on core")
    );
    assert_eq!(plan["full"], false);
    assert_eq!(plan["left_out"][0]["command"], "cargo test --workspace");

    w.write("crates/app/src/main.rs", "fn main() { }\n");
    w.git(&["add", "-A"]);
    w.git(&["commit", "-q", "-m", "x"]);
    w.write("crates/app/src/main.rs", "fn main() {}\n");
    assert_eq!(commands(&w.json(&["plan"], 0)), ["cargo test -p app"]);
}

#[test]
fn plan_falls_back_to_the_full_suite_and_says_why() {
    let w = World::new(WORKSPACE);
    w.write("scripts/deploy.sh", "echo hi\n");
    let plan = w.json(&["plan"], 0);
    assert_eq!(plan["full"], true);
    assert_eq!(commands(&plan), ["cargo test --workspace"]);
    assert_eq!(plan["unmatched"], json!(["scripts/deploy.sh"]));
    w.ube()
        .arg("plan")
        .assert()
        .stdout(predicate::str::contains(
            "no check is linked to scripts/deploy.sh",
        ));
}

/// An npm package whose test script is a stub: it fails the way vitest does
/// while `$HOME/fail` exists.
fn npm_world() -> World {
    let w = World::new(&[
        (
            "web/package.json",
            r#"{"scripts": {"test": "vitest run", "dev": "vite"}}"#,
        ),
        ("web/src/money.ts", "export const cents = 1;\n"),
        ("web/src/money.test.ts", "test('x', () => {});\n"),
    ]);
    w.stub(
        "npm",
        &format!(
            "echo \"npm $*\" >> {calls}\nif [ -f {fail} ]; then\n  echo ' FAIL  src/money.test.ts > formatCents > rounds half even 3ms'\n  echo \"AssertionError: expected '1.01' to be '1.02'\"\n  echo ' ❯ src/money.test.ts:14:22'\n  exit 1\nfi\necho 'Test Files  1 passed'",
            calls = w.dir.path().join("npm-calls").display(),
            fail = w.dir.path().join("fail").display()
        ),
    );
    w
}

#[test]
fn run_reports_failures_briefly_and_ties_results_to_the_tree() {
    let w = npm_world();
    std::fs::write(w.dir.path().join("fail"), "").unwrap();
    w.write("web/src/money.ts", "export const cents = 2;\n");

    let out = w
        .ube()
        .arg("run")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("FAIL  npm test (in web)"))
        .stdout(predicate::str::contains(
            "src/money.test.ts:14  formatCents > rounds half even",
        ))
        .stdout(predicate::str::contains(
            "rerun: npx vitest run src/money.test.ts -t \"rounds half even\"",
        ))
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8(out).unwrap().lines().count() < 12);

    // Not done: the check failed on this tree.
    w.ube()
        .arg("done")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("failed on this tree: npm test"));

    // Fixed: --rerun-failed runs just that, and passes.
    std::fs::remove_file(w.dir.path().join("fail")).unwrap();
    let body = w.json(&["run", "--rerun-failed"], 0);
    assert_eq!(body["passed"], true);
    assert_eq!(body["results"][0]["command"], "npm test");

    // .ube/ holds the results and ignores itself.
    assert!(w.repo().join(".ube/.gitignore").exists());
    assert_eq!(w.git(&["status", "--porcelain"]), " M web/src/money.ts\n");

    // An edit after the run makes the proof stale.
    w.write("web/src/money.ts", "export const cents = 3;\n");
    w.ube()
        .arg("done")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("not run on this tree"));
}

#[test]
fn run_takes_one_command_and_shows_unreadable_output() {
    let w = npm_world();
    w.ube()
        .args(["run", "--", "echo 'something odd happened' >&2; exit 3"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "no failure could be read from the output",
        ))
        .stdout(predicate::str::contains("something odd happened"));
}

#[test]
fn done_reads_matchas_answers() {
    let w = npm_world();
    w.write("web/src/money.ts", "export const cents = 2;\n");
    w.ube().arg("run").assert().code(0);
    // A matcha stand-in: recipe finds a missing companion, drift a doc
    // claim about the changed file, ledger nothing.
    w.stub(
        "matcha",
        r#"case "$*" in
  *"recipe --check"*) echo '{"missing":[{"changed":"web/src/money.ts","missing":"web/src/money.test.ts"}]}'; exit 1;;
  *drift*) echo '{"failing":[{"doc":"docs/a.md","line":3,"claim":"web/src/money.ts","reason":"gone"},{"doc":"docs/b.md","line":1,"claim":"Other","reason":""}]}'; exit 1;;
  *ledger*) echo '{"flagged":[]}';;
  *) exit 0;;
esac"#,
    );
    let body = w.json(&["done"], 1);
    let state = |name: &str| {
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["name"] == name)
            .unwrap()["state"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(state("checks"), "ok");
    assert_eq!(state("tests"), "warn");
    assert_eq!(state("companions"), "fail");
    assert_eq!(state("docs"), "fail");
    assert_eq!(state("claims"), "ok");
    let docs = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "docs")
        .unwrap();
    assert!(
        docs["detail"]
            .as_str()
            .unwrap()
            .contains("1 other failing claims"),
        "{docs}"
    );
}

#[test]
fn done_says_what_it_couldnt_check_without_matcha() {
    let w = npm_world();
    w.write("web/src/money.test.ts", "test('y', () => {});\n");
    w.ube().arg("run").assert().code(0);
    let body = w.json(&["done"], 0);
    let companions = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "companions")
        .unwrap();
    assert_eq!(companions["state"], "unknown");
    assert_eq!(body["done"], true);
}

#[test]
fn mutate_finds_a_line_no_test_proves() {
    // The test checks the threshold, but nothing checks the bonus.
    let w = World::new(&[
        ("Makefile", "test:\n\tpython3 tests/check.py\n"),
        ("src/calc.py", "def ok(n):\n    return True\n"),
        (
            "tests/check.py",
            "import sys\nsys.path.insert(0, 'src')\nimport calc\nassert calc.ok(10) is True\nassert calc.ok(9) is False\n",
        ),
    ]);
    w.write(
        "src/calc.py",
        "def ok(n):\n    return n >= 10\n\ndef bonus(n):\n    return n + 5\n",
    );
    let body = w.json(&["mutate"], 1);
    assert_eq!(body["baseline_passed"], true, "{body}");
    let by_change = |c: &str| {
        body["mutants"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["change"] == c)
            .unwrap()
            .clone()
    };
    assert_eq!(by_change(">= to <")["killed"], true);
    assert_eq!(by_change(">= to <")["by"], "make test");
    assert_eq!(by_change("+ to -")["killed"], false);
    assert_eq!(by_change("+ to -")["line"], 5);
    // The working tree is untouched.
    assert!(
        std::fs::read_to_string(w.repo().join("src/calc.py"))
            .unwrap()
            .contains("n >= 10")
    );
    assert!(
        !w.git(&["status", "--porcelain"]).contains(".ube-"),
        "no leftovers"
    );
}

#[test]
fn the_stop_hook_asks_for_proof_once() {
    let w = npm_world();
    let input = |active: bool| {
        json!({"cwd": w.repo(), "hook_event_name": "Stop", "stop_hook_active": active}).to_string()
    };
    // No changes: stay out of the way.
    w.ube()
        .args(["hook", "claude-code"])
        .write_stdin(input(false))
        .assert()
        .success()
        .stdout("");

    w.write("web/src/money.ts", "export const cents = 2;\n");
    let out = w
        .ube()
        .args(["hook", "claude-code"])
        .write_stdin(input(false))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(body["decision"], "block");
    assert!(body["reason"].as_str().unwrap().contains("npm test"));

    // Already continuing because of it: let the agent stop.
    w.ube()
        .args(["hook", "claude-code"])
        .write_stdin(input(true))
        .assert()
        .success()
        .stdout("");

    // Proven: stop freely.
    w.ube().arg("run").assert().code(0);
    w.ube()
        .args(["hook", "claude-code"])
        .write_stdin(input(false))
        .assert()
        .success()
        .stdout("");
}

#[test]
fn mcp_serves_plan_run_and_done() {
    let w = npm_world();
    w.write("web/src/money.ts", "export const cents = 2;\n");
    let msgs = [
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"plan","arguments":{}}}),
    ];
    let input: String = msgs.iter().map(|m| format!("{m}\n")).collect();
    let out = w
        .ube()
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
    let names: Vec<&str> = replies[0]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["plan", "run", "done"]);
    let plan: Value =
        serde_json::from_str(replies[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(commands(&plan), ["npm test"]);
}

#[test]
fn outside_a_repo_it_says_how_to_fix_it() {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("ube")
        .unwrap()
        .current_dir(dir.path())
        .arg("plan")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("pass --repo <path>"));
}
