//! census against a real git repo: what it counts and what it leaves out.

// Test helpers may panic; clippy's allow-unwrap-in-tests only covers #[test] fns.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::Command as Process;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
    let git = |args: &[&str]| {
        assert!(
            Process::new("git")
                .arg("-C")
                .arg(dir.path())
                .args([
                    "-c",
                    "user.name=T",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false"
                ])
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "init"]);
    dir
}

fn census(dir: &Path, args: &[&str]) -> Value {
    let out = Command::cargo_bin("matcha")
        .unwrap()
        .arg("--repo")
        .arg(dir)
        .arg("census")
        .args(args)
        .arg("--json")
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).unwrap()
}

const RUST: &[(&str, &str)] = &[
    (
        "src/lib.rs",
        "pub fn a() { x.unwrap(); }\n\n#[cfg(test)]\nmod tests {\n    fn t() { y.unwrap(); }\n}\n",
    ),
    ("tests/it.rs", "fn it() { z.unwrap(); }\n"),
    ("Cargo.lock", "# unwrap unwrap unwrap\nname = \"x\"\n"),
    (
        "frontend/package-lock.json",
        "{\"license\": \"MIT\", \"x\": {\"license\": \"MIT\"}}\n",
    ),
    ("README.md", "MIT license.\n"),
];

#[test]
fn says_how_many_matches_are_in_tests_and_can_leave_them_out() {
    let repo = repo(RUST);
    let all = census(repo.path(), &["--alt", r"\.unwrap\(\)"]);
    let t = &all["alternatives"][0];
    assert_eq!(
        (
            t["files"].as_u64(),
            t["matches"].as_u64(),
            t["test_files"].as_u64()
        ),
        (Some(2), Some(3), Some(1))
    );

    // --no-tests drops tests/ and Rust's inline test module.
    let prod = census(repo.path(), &["--alt", r"\.unwrap\(\)", "--no-tests"]);
    let t = &prod["alternatives"][0];
    assert_eq!(
        (t["files"].as_u64(), t["matches"].as_u64()),
        (Some(1), Some(1))
    );
    assert_eq!(t["examples"][0]["path"], "src/lib.rs");
}

#[test]
fn lockfiles_dont_count() {
    let repo = repo(RUST);
    let body = census(repo.path(), &["--alt", r"\blicense\b"]);
    assert_eq!(body["alternatives"][0]["files"], 1);
    assert_eq!(body["alternatives"][0]["examples"][0]["path"], "README.md");
    let unwraps = census(repo.path(), &["--alt", "unwrap unwrap"]);
    assert_eq!(unwraps["alternatives"][0]["files"], 0);
}

#[test]
fn a_pattern_narrower_than_its_word_says_so() {
    let repo = repo(&[("web/Form.tsx", "const f = useForm<Values>({});\n")]);
    let body = census(repo.path(), &["--alt", r"useForm\("]);
    let t = &body["alternatives"][0];
    assert_eq!(t["files"], 0);
    assert_eq!(t["near_miss"]["word"], "useForm");
    assert_eq!(t["near_miss"]["example"]["path"], "web/Form.tsx");

    Command::cargo_bin("matcha")
        .unwrap()
        .arg("--repo")
        .arg(repo.path())
        .args(["census", "--alt", r"useForm\("])
        .assert()
        .stdout(predicate::str::contains("but `useForm` alone is in 1 file"));
}
