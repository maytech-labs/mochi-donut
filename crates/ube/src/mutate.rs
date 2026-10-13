//! `ube mutate`: can the tests for this change fail at all? Makes small
//! changes to changed lines, one at a time, in a temporary copy of the repo,
//! and runs the change's tests against each. A change no test notices is a
//! survivor: that line isn't proven by any test (docs/ube/design.md,
//! "ube mutate").

use std::path::{Path, PathBuf};
use std::time::Duration;

use mochi_common::paths::is_test;
use mochi_index::{AddedLine, RepoRoot};
use regex::Regex;
use serde::Serialize;

use crate::plan::Check;
use crate::run::run_check;

#[derive(Debug, Clone, Serialize)]
pub struct Mutant {
    pub file: String,
    pub line: usize,
    /// What changed: `== to !=`.
    pub change: String,
    pub original: String,
    pub mutated: String,
    /// Whether a check failed with it in place. `None` when it wasn't run.
    pub killed: Option<bool>,
    /// The check that failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    /// Test checks run against each mutant.
    pub checks: Vec<String>,
    /// Whether they passed on the unmutated copy. If not, nothing was mutated.
    pub baseline_passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_failure: Option<String>,
    pub candidates: usize,
    pub mutants: Vec<Mutant>,
    pub killed: usize,
    pub survived: usize,
}

/// Each operator: what it looks for, what it puts instead, and its name.
const OPERATORS: &[(&str, &str)] = &[
    (" == ", " != "),
    (" != ", " == "),
    (" <= ", " > "),
    (" >= ", " < "),
    (" < ", " >= "),
    (" > ", " <= "),
    (" && ", " || "),
    (" || ", " && "),
    (" + ", " - "),
    (" - ", " + "),
];

const CODE: &[&str] = &[
    "rs", "go", "py", "ts", "tsx", "js", "jsx", "mjs", "java", "kt", "rb", "swift", "c", "cc",
    "cpp", "cs", "php", "scala",
];

/// Every mutation of the added lines in non-test code files under `paths`
/// (all of them when empty).
pub fn candidates(added: &[AddedLine], paths: &[String]) -> Vec<Mutant> {
    let bool_lit = Regex::new(r"\b(true|false|True|False)\b").ok();
    let mut out = Vec::new();
    for a in added {
        let ext = a.path.rsplit_once('.').map_or("", |(_, e)| e);
        if !CODE.contains(&ext)
            || is_test(&a.path)
            || (!paths.is_empty() && !paths.iter().any(|p| a.path.starts_with(p.as_str())))
        {
            continue;
        }
        let t = a.text.trim_start();
        if t.is_empty()
            || [
                "//", "#", "*", "/*", "--", "use ", "import ", "from ", "package ", "mod ",
            ]
            .iter()
            .any(|p| t.starts_with(p))
        {
            continue;
        }
        let mut push = |change: String, mutated: String| {
            out.push(Mutant {
                file: a.path.clone(),
                line: a.line,
                change,
                original: a.text.clone(),
                mutated,
                killed: None,
                by: None,
            });
        };
        for (from, to) in OPERATORS {
            if let Some(i) = outside_strings(&a.text, from) {
                let mutated = format!("{}{to}{}", &a.text[..i], &a.text[i + from.len()..]);
                push(format!("{} to {}", from.trim(), to.trim()), mutated);
            }
        }
        if let Some(re) = &bool_lit
            && let Some(m) = re.find(&a.text)
            && outside_strings(&a.text, m.as_str()) == Some(m.start())
        {
            let flipped = match m.as_str() {
                "true" => "false",
                "false" => "true",
                "True" => "False",
                _ => "True",
            };
            let mutated = format!("{}{flipped}{}", &a.text[..m.start()], &a.text[m.end()..]);
            push(format!("{} to {flipped}", m.as_str()), mutated);
        }
    }
    out
}

/// The first `needle` in `line` that isn't inside a string literal.
fn outside_strings(line: &str, needle: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        if c == '"' || c == '\'' || c == '`' {
            quote = Some(c);
            continue;
        }
        if line[i..].starts_with(needle) {
            return Some(i);
        }
    }
    None
}

/// Spreads `max` picks evenly over `all`, so a big change isn't judged by
/// its first few lines.
pub fn spread(all: Vec<Mutant>, max: usize) -> Vec<Mutant> {
    if all.len() <= max || max == 0 {
        return all;
    }
    let step = all.len() as f64 / max as f64;
    (0..max)
        .filter_map(|i| all.get((i as f64 * step) as usize).cloned())
        .collect()
}

/// Runs `checks` against each mutant in a temporary copy of the repo.
pub fn run(
    root: &RepoRoot,
    checks: &[Check],
    mut mutants: Vec<Mutant>,
    candidates: usize,
    timeout: Duration,
) -> Result<Report, String> {
    let copy = Copy::make(root)?;
    // Builds in the copy reuse the repo's own Cargo cache, under its ignored target/.
    let target = root.path().join("target").join("ube-mutate");
    let target = target.display().to_string();
    let env: Vec<(&str, &str)> = vec![("CARGO_TARGET_DIR", target.as_str())];
    let names: Vec<String> = checks.iter().map(|c| c.command.clone()).collect();
    let run_all = |dir: &Path| -> Option<String> {
        checks
            .iter()
            .find(|c| !run_check(dir, c, timeout, &env).passed)
            .map(|c| c.command.clone())
    };
    if let Some(failed) = run_all(&copy.dir) {
        return Ok(Report {
            checks: names,
            baseline_passed: false,
            baseline_failure: Some(failed),
            candidates,
            mutants,
            killed: 0,
            survived: 0,
        });
    }
    for m in &mut mutants {
        let path = copy.dir.join(&m.file);
        let original =
            std::fs::read_to_string(&path).map_err(|e| format!("couldn't read {}: {e}", m.file))?;
        let mut lines: Vec<&str> = original.split('\n').collect();
        let Some(slot) = lines.get_mut(m.line.saturating_sub(1)) else {
            continue;
        };
        *slot = &m.mutated;
        std::fs::write(&path, lines.join("\n")).map_err(|e| e.to_string())?;
        let failed = run_all(&copy.dir);
        std::fs::write(&path, &original).map_err(|e| e.to_string())?;
        m.killed = Some(failed.is_some());
        m.by = failed;
    }
    let killed = mutants.iter().filter(|m| m.killed == Some(true)).count();
    let survived = mutants.iter().filter(|m| m.killed == Some(false)).count();
    Ok(Report {
        checks: names,
        baseline_passed: true,
        baseline_failure: None,
        candidates,
        mutants,
        killed,
        survived,
    })
}

/// A copy of the repo's tracked and untracked files in a temporary
/// directory, removed when dropped. Installed dependencies (`node_modules`)
/// are linked, not copied.
struct Copy {
    tmp: PathBuf,
    dir: PathBuf,
}

impl Copy {
    fn make(root: &RepoRoot) -> Result<Copy, String> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let tmp = std::env::temp_dir().join(format!("ube-mutate-{}-{nanos}", std::process::id()));
        let dir = tmp.join("repo");
        let copy = Copy { tmp, dir };
        let mut files = root.tracked_files().map_err(|e| e.to_string())?;
        files.extend(root.changed_files(None).map_err(|e| e.to_string())?);
        files.sort();
        files.dedup();
        for f in &files {
            let from = root.path().join(f);
            if !from.is_file() {
                continue;
            }
            let to = copy.dir.join(f);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::copy(&from, &to).map_err(|e| format!("couldn't copy {f}: {e}"))?;
        }
        #[cfg(unix)]
        for f in files.iter().filter(|f| f.ends_with("package.json")) {
            let dir = f.rsplit_once('/').map_or("", |(d, _)| d);
            let modules = root.path().join(dir).join("node_modules");
            if modules.is_dir() {
                let _ =
                    std::os::unix::fs::symlink(&modules, copy.dir.join(dir).join("node_modules"));
            }
        }
        Ok(copy)
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn added(path: &str, line: usize, text: &str) -> AddedLine {
        AddedLine {
            path: path.into(),
            line,
            text: text.into(),
        }
    }

    #[test]
    fn mutates_operators_and_booleans_outside_strings() {
        let lines = [
            added("src/a.rs", 3, "    if n >= limit && ready { return true; }"),
            added("src/a.rs", 4, "    let s = \"a == b\";"),
            added("src/a.rs", 5, "    // if a == b"),
            added("tests/a.rs", 1, "assert!(x == y);"),
            added("README.md", 1, "a == b"),
        ];
        let m = candidates(&lines, &[]);
        let changes: Vec<&str> = m.iter().map(|m| m.change.as_str()).collect();
        assert_eq!(changes, [">= to <", "&& to ||", "true to false"]);
        assert_eq!(m[0].mutated, "    if n < limit && ready { return true; }");
        assert!(m.iter().all(|m| m.line == 3));
    }

    #[test]
    fn spreads_picks_across_the_change() {
        let all: Vec<Mutant> = (1..=10)
            .map(|i| Mutant {
                file: "a.rs".into(),
                line: i,
                change: String::new(),
                original: String::new(),
                mutated: String::new(),
                killed: None,
                by: None,
            })
            .collect();
        let lines: Vec<usize> = spread(all, 3).iter().map(|m| m.line).collect();
        assert_eq!(lines, [1, 4, 7]);
    }
}
