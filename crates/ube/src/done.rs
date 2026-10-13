//! `ube done`: is this change done? A checklist for the exact tree: the
//! planned checks passed on it, changed code has test changes, and matcha
//! finds no missing companion, no doc made false, and no assumed claim
//! written into a doc (docs/ube/design.md, "ube done").

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use mochi_common::paths::is_test;
use mochi_common::proc::run;
use serde::Serialize;
use serde_json::Value;

use crate::detect::Project;
use crate::plan::Plan;
use crate::store::CheckResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Ok,
    /// Something to look at that doesn't block: a code change with no test change.
    Warn,
    Fail,
    /// It couldn't be checked: matcha isn't installed.
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub name: &'static str,
    pub state: State,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

fn item(name: &'static str, state: State, detail: impl Into<String>, fix: Option<&str>) -> Item {
    Item {
        name,
        state,
        detail: detail.into(),
        fix: fix.map(str::to_owned),
    }
}

/// The checklist for `plan`'s change, given what ran on this tree.
/// `inline_tested` are changed files whose own test code changed, such as
/// a Rust file's `#[cfg(test)]` module.
pub fn done(
    root: &Path,
    project: &Project,
    plan: &Plan,
    results: &BTreeMap<String, CheckResult>,
    base: Option<&str>,
    inline_tested: &[String],
) -> Vec<Item> {
    if plan.changed.is_empty() {
        return vec![item(
            "change",
            State::Ok,
            "No changes against the base: nothing to prove.",
            None,
        )];
    }
    let present: Vec<String> = plan
        .changed
        .iter()
        .filter(|f| root.join(f).is_file())
        .cloned()
        .collect();
    let mut items = vec![
        checks_item(plan, results),
        tests_item(project, &present, inline_tested),
    ];
    items.push(matcha_item(
        root,
        "companions",
        &["recipe", "--check"],
        |v| {
            let missing: Vec<String> = v["missing"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|m| {
                    format!(
                        "{} (usually changes with {})",
                        m["missing"].as_str().unwrap_or("?"),
                        m["changed"].as_str().unwrap_or("?")
                    )
                })
                .collect();
            if missing.is_empty() {
                (
                    State::Ok,
                    "No file that usually changes with these is missing.".into(),
                    None,
                )
            } else {
                (
                    State::Fail,
                    format!("Missing: {}", missing.join("; ")),
                    Some("Change them too, or check why not: matcha recipe --check"),
                )
            }
        },
    ));
    let changed = plan.changed.clone();
    items.push(matcha_item(root, "docs", &["drift"], move |v| {
        let ours: Vec<String> = v["failing"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|f| touches(f, &changed))
            .map(|f| {
                format!(
                    "{}:{} {}",
                    f["doc"].as_str().unwrap_or("?"),
                    f["line"],
                    f["claim"].as_str().unwrap_or("?")
                )
            })
            .collect();
        let others = v["failing"].as_array().map_or(0, Vec::len) - ours.len();
        let elsewhere = if others > 0 {
            format!(" ({others} other failing claims don't involve this change.)")
        } else {
            String::new()
        };
        if ours.is_empty() {
            (
                State::Ok,
                format!("No doc claim about these files fails.{elsewhere}"),
                None,
            )
        } else {
            (
                State::Fail,
                format!(
                    "Doc claims this change made false: {}.{elsewhere}",
                    ours.join("; ")
                ),
                Some("Update the docs: matcha drift"),
            )
        }
    }));
    let mut ledger_args = vec!["ledger", "check", "--diff"];
    if let Some(b) = base {
        ledger_args.extend(["--base", b]);
    }
    items.push(matcha_item(root, "claims", &ledger_args, |v| {
        let flagged: Vec<String> = v["flagged"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| format!("{}:{} states assumed #{}", f["path"].as_str().unwrap_or("?"), f["line"], f["id"]))
            .collect();
        if flagged.is_empty() {
            (State::Ok, "No assumed claim is written into a doc or comment.".into(), None)
        } else {
            (State::Fail, flagged.join("; "), Some("Verify each (matcha ledger verify <id> --evidence ...) or mark it unverified in the text"))
        }
    }));
    items
}

fn checks_item(plan: &Plan, results: &BTreeMap<String, CheckResult>) -> Item {
    let mut failed = Vec::new();
    let mut not_run = Vec::new();
    for c in &plan.checks {
        match results.get(&c.id()) {
            Some(r) if r.passed => {}
            Some(_) => failed.push(c.command.clone()),
            None => not_run.push(c.command.clone()),
        }
    }
    let scope = if plan.full {
        "the full suite"
    } else {
        "the selected checks"
    };
    if failed.is_empty() && not_run.is_empty() {
        let left = if plan.left_out.is_empty() {
            String::new()
        } else {
            format!(
                " Left out: {}.",
                plan.left_out
                    .iter()
                    .map(|l| l.command.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        return item(
            "checks",
            State::Ok,
            format!(
                "All {} of {scope} passed on this exact tree.{left}",
                plan.checks.len()
            ),
            None,
        );
    }
    let mut parts = Vec::new();
    if !failed.is_empty() {
        parts.push(format!("failed on this tree: {}", failed.join(", ")));
    }
    if !not_run.is_empty() {
        parts.push(format!(
            "not run on this tree (or the code changed since): {}",
            not_run.join(", ")
        ));
    }
    item("checks", State::Fail, parts.join("; "), Some("ube run"))
}

/// Code changed in a unit (a package, else a top-level directory) with no
/// test file changed in the same unit.
fn tests_item(project: &Project, changed: &[String], inline_tested: &[String]) -> Item {
    const CODE: &[&str] = &[
        "rs", "go", "py", "ts", "tsx", "js", "jsx", "mjs", "cjs", "java", "kt", "rb", "swift", "c",
        "cc", "cpp", "h", "hpp", "cs", "php", "scala", "zig",
    ];
    let units: Vec<&str> = project
        .cargo
        .iter()
        .map(|p| p.dir.as_str())
        .chain(project.npm.iter().map(|p| p.dir.as_str()))
        .chain(project.go.iter().map(String::as_str))
        .chain(project.python.iter().map(String::as_str))
        .collect();
    let unit_of = |f: &str| -> String {
        units
            .iter()
            .filter(|u| !u.is_empty() && f.strip_prefix(**u).is_some_and(|r| r.starts_with('/')))
            .max_by_key(|u| u.len())
            .map_or_else(
                || f.split('/').next().unwrap_or(f).to_owned(),
                |u| (*u).to_owned(),
            )
    };
    let mut code: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut tested: Vec<String> = Vec::new();
    for f in changed {
        let ext = f.rsplit_once('.').map_or("", |(_, e)| e);
        if !CODE.contains(&ext) {
            continue;
        }
        if is_test(f) || inline_tested.contains(f) {
            tested.push(unit_of(f));
        } else {
            code.entry(unit_of(f)).or_default().push(f);
        }
    }
    let untested: Vec<String> = code
        .iter()
        .filter(|(unit, _)| !tested.contains(unit))
        .map(|(unit, files)| format!("{unit} ({})", files.join(", ")))
        .collect();
    if untested.is_empty() {
        item(
            "tests",
            State::Ok,
            "Every unit with changed code has a test change too.",
            None,
        )
    } else {
        item(
            "tests",
            State::Warn,
            format!("Code changed with no test change: {}", untested.join("; ")),
            Some(
                "Add or update an integration test that pins the behavior, or say why existing tests already do",
            ),
        )
    }
}

/// Whether a drift failure involves a changed file: the doc itself, or a
/// claim naming a changed file or its name.
fn touches(f: &Value, changed: &[String]) -> bool {
    let doc = f["doc"].as_str().unwrap_or("");
    let text = format!(
        "{} {}",
        f["claim"].as_str().unwrap_or(""),
        f["reason"].as_str().unwrap_or("")
    );
    changed.iter().any(|c| {
        let name = c.rsplit('/').next().unwrap_or(c);
        c == doc || text.contains(c.as_str()) || (name.len() > 3 && text.contains(name))
    })
}

/// Runs `matcha <args> --json --repo root` and judges its output.
fn matcha_item(
    root: &Path,
    name: &'static str,
    args: &[&str],
    judge: impl FnOnce(&Value) -> (State, String, Option<&'static str>) + 'static,
) -> Item {
    let mut cmd = Command::new("matcha");
    cmd.arg("--json").arg("--repo").arg(root).args(args);
    let out = match run(cmd, Duration::from_secs(300)) {
        Ok(o) => o,
        Err(_) => {
            return item(
                name,
                State::Unknown,
                "matcha isn't installed, so this wasn't checked.",
                Some("Install matcha (make install) to check companions, docs, and claims"),
            );
        }
    };
    match serde_json::from_str::<Value>(out.stdout.trim()) {
        Ok(v) if matches!(out.code, Some(0 | 1)) => {
            let (state, detail, fix) = judge(&v);
            item(name, state, detail, fix)
        }
        _ => item(
            name,
            State::Unknown,
            format!(
                "matcha {} didn't answer: {}",
                args.join(" "),
                out.stderr.trim().lines().next().unwrap_or("no output")
            ),
            None,
        ),
    }
}

/// Changed files whose added lines fall inside their own test code: after a
/// Rust file's `#[cfg(test)]`. Other languages keep tests in separate files.
pub fn inline_tested(root: &Path, added: &[mochi_index::AddedLine]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for a in added.iter().filter(|a| a.path.ends_with(".rs")) {
        if out.contains(&a.path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&a.path)) else {
            continue;
        };
        let start = text
            .lines()
            .position(|l| l.trim_start().starts_with("#[cfg(test)]"))
            .map(|i| i + 1);
        if start.is_some_and(|s| a.line > s) {
            out.push(a.path.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::CargoPkg;
    use crate::plan::Check;

    fn result(passed: bool) -> CheckResult {
        CheckResult {
            command: String::new(),
            dir: String::new(),
            passed,
            exit: Some(i32::from(!passed)),
            secs: 1.0,
            failures: vec![],
            tail: vec![],
            at: String::new(),
        }
    }

    fn plan_of(cmds: &[&str]) -> Plan {
        Plan {
            changed: vec!["a.rs".into()],
            checks: cmds
                .iter()
                .map(|c| Check {
                    command: (*c).into(),
                    dir: String::new(),
                    reason: String::new(),
                    tests: true,
                })
                .collect(),
            left_out: vec![],
            unmatched: vec![],
            full: false,
        }
    }

    #[test]
    fn checks_must_have_passed_on_this_tree() {
        let p = plan_of(&["cargo test -p a", "cargo test -p b"]);
        let mut r = BTreeMap::new();
        r.insert("cargo test -p a".to_owned(), result(true));
        let i = checks_item(&p, &r);
        assert_eq!(i.state, State::Fail);
        assert!(i.detail.contains("not run on this tree"));
        assert!(i.detail.contains("cargo test -p b"));
        r.insert("cargo test -p b".to_owned(), result(false));
        assert!(
            checks_item(&p, &r)
                .detail
                .contains("failed on this tree: cargo test -p b")
        );
        r.insert("cargo test -p b".to_owned(), result(true));
        assert_eq!(checks_item(&p, &r).state, State::Ok);
    }

    #[test]
    fn code_without_a_test_change_is_a_warning() {
        let project = Project {
            cargo: vec![CargoPkg {
                name: "y".into(),
                dir: "crates/yuzu".into(),
                deps: vec![],
            }],
            ..Project::default()
        };
        let i = tests_item(
            &project,
            &["crates/yuzu/src/wait.rs".into(), "docs/a.md".into()],
            &[],
        );
        assert_eq!(i.state, State::Warn);
        assert!(i.detail.contains("crates/yuzu (crates/yuzu/src/wait.rs)"));
        let i = tests_item(
            &project,
            &[
                "crates/yuzu/src/wait.rs".into(),
                "crates/yuzu/tests/outside.rs".into(),
            ],
            &[],
        );
        assert_eq!(i.state, State::Ok);
        // A test added in the file's own test module counts.
        let wait = vec!["crates/yuzu/src/wait.rs".to_owned()];
        assert_eq!(tests_item(&project, &wait, &wait).state, State::Ok);
    }

    #[test]
    fn drift_failures_count_only_when_they_involve_the_change() {
        let changed = vec![
            "crates/yuzu/src/wait.rs".to_owned(),
            "docs/yuzu/usage.md".to_owned(),
        ];
        let f =
            |doc: &str, claim: &str| serde_json::json!({"doc": doc, "claim": claim, "reason": ""});
        assert!(touches(&f("docs/yuzu/usage.md", "x"), &changed));
        assert!(touches(
            &f("README.md", "crates/yuzu/src/wait.rs"),
            &changed
        ));
        assert!(touches(&f("README.md", "wait.rs"), &changed));
        assert!(!touches(&f("README.md", "ClientId"), &changed));
    }
}
