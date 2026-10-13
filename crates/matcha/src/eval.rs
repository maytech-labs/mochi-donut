//! `matcha eval`: score a command's answers against labeled ones. Each case
//! runs any command that prints JSON in a repo pinned to a commit, finds the
//! findings in its output, and compares them with findings a person marked
//! real or false. The result is precision and recall with the evidence, so a
//! heuristic change can be judged by numbers instead of by eye
//! (docs/decisions/0024-matcha-eval.md).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub cases: Vec<Case>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    /// A local path (`~` allowed, relative to the spec file) or a git URL.
    pub repo: String,
    /// The commit to check out in a temporary clone. Without it, the command
    /// runs in the repo as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    /// The command and its arguments. It must print JSON on stdout.
    /// `matcha` and `yuzu` mean the binaries beside this one.
    pub run: Vec<String>,
    /// A JSON pointer to the array of findings, such as `/failing`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub findings: Option<String>,
    /// How to name a finding: `{doc}:{line} {claim}`, from its fields.
    /// Default: the finding's JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Findings a person judged right.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub real: Vec<String>,
    /// Findings a person judged wrong: false alarms.
    #[serde(default, rename = "false", skip_serializing_if = "Vec::is_empty")]
    pub false_: Vec<String>,
    /// Findings nobody has judged yet (written by `--record`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unlabeled: Vec<String>,
    /// Exact values the output must have, by JSON pointer.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub expect: serde_json::Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_precision: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_recall: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct CaseResult {
    pub name: String,
    pub ok: bool,
    /// Why the case couldn't run, if it couldn't.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub flagged: usize,
    pub real_found: Vec<String>,
    pub false_found: Vec<String>,
    /// Labeled real, not found.
    pub missed: Vec<String>,
    /// Found, not labeled.
    pub unlabeled: Vec<String>,
    /// Real out of labeled findings, when any are labeled.
    pub precision: Option<f64>,
    /// Found out of labeled real, when any are labeled real.
    pub recall: Option<f64>,
    /// Whether the case has findings to score, or only `expect` values.
    pub scored: bool,
    pub expected: usize,
    /// `expect` entries that didn't match: pointer, wanted, got.
    pub mismatches: Vec<Mismatch>,
    /// Gates that failed: "precision 80% < 90%".
    pub failures: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Mismatch {
    pub pointer: String,
    pub expected: Value,
    pub got: Value,
}

/// Reads a spec file. Unknown fields are errors, so a typo can't quietly
/// turn a gate off.
pub fn load(path: &Path) -> Result<Spec, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|e| format!("{} isn't a valid eval spec: {e}", path.display()))
}

pub fn save(path: &Path, spec: &Spec) -> Result<(), String> {
    let text = serde_json::to_string_pretty(spec).map_err(|e| e.to_string())?;
    std::fs::write(path, text + "\n").map_err(|e| format!("couldn't write {}: {e}", path.display()))
}

/// Runs one case. `spec_dir` resolves relative repo paths; `home` expands `~`.
pub fn run_case(case: &Case, spec_dir: &Path, home: &Path) -> CaseResult {
    let mut result = CaseResult {
        name: case.name.clone(),
        ok: false,
        error: None,
        flagged: 0,
        real_found: Vec::new(),
        false_found: Vec::new(),
        missed: Vec::new(),
        unlabeled: Vec::new(),
        precision: None,
        recall: None,
        scored: case.findings.is_some(),
        expected: case.expect.len(),
        mismatches: Vec::new(),
        failures: Vec::new(),
    };
    let output = match checkout(case, spec_dir, home).and_then(|dir| execute(case, &dir.0)) {
        Ok(v) => v,
        Err(e) => {
            result.error = Some(e);
            return result;
        }
    };
    score(case, &output, &mut result);
    result
}

/// Where a case runs, and the temp dir that holds it, deleted on drop.
struct Workdir(PathBuf, Option<PathBuf>);

impl Drop for Workdir {
    fn drop(&mut self) {
        if let Some(tmp) = &self.1 {
            let _ = std::fs::remove_dir_all(tmp);
        }
    }
}

fn checkout(case: &Case, spec_dir: &Path, home: &Path) -> Result<Workdir, String> {
    let repo = if case.repo.contains("://") || case.repo.starts_with("git@") {
        case.repo.clone()
    } else {
        let p = match case.repo.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => spec_dir.join(&case.repo),
        };
        p.display().to_string()
    };
    let Some(rev) = &case.rev else {
        return Ok(Workdir(PathBuf::from(repo), None));
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let tmp = std::env::temp_dir().join(format!("matcha-eval-{}-{nanos}", std::process::id()));
    let dir = tmp.join("repo");
    // Owned from here, so a failed clone still gets cleaned up.
    let work = Workdir(dir.clone(), Some(tmp));
    let git = |args: &[&str]| -> Result<(), String> {
        let out = Command::new("git")
            .args(args)
            .output()
            .map_err(|e| format!("couldn't run git: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "`git {}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    };
    let dir_s = dir.display().to_string();
    // --shared borrows a local repo's objects instead of copying them.
    let shared = if repo.contains("://") || repo.starts_with("git@") {
        "--filter=blob:none"
    } else {
        "--shared"
    };
    git(&["clone", "-q", "--no-checkout", shared, &repo, &dir_s])?;
    git(&["-C", &dir_s, "checkout", "-q", "--detach", rev])?;
    Ok(work)
}

fn execute(case: &Case, dir: &Path) -> Result<Value, String> {
    let (program, args) = case
        .run
        .split_first()
        .ok_or("`run` is empty: give the command and its arguments")?;
    // `matcha` and `yuzu` are the binaries beside this one, so an eval scores
    // this build, not whatever is on PATH.
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(program)))
        .filter(|p| matches!(program.as_str(), "matcha" | "yuzu") && p.exists());
    let mut cmd = Command::new(sibling.as_deref().unwrap_or(Path::new(program)));
    let out = cmd
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("couldn't run {program}: {e}"))?;
    // 0 and 1 are both answers; 2 and up mean it didn't run as meant.
    if out.status.code().is_none_or(|c| c > 1) {
        return Err(format!(
            "`{}` exited {:?}: {}",
            case.run.join(" "),
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| {
        format!(
            "`{}` didn't print JSON ({e}). Add its --json flag to `run`.",
            case.run.join(" ")
        )
    })
}

fn score(case: &Case, output: &Value, r: &mut CaseResult) {
    for (pointer, expected) in &case.expect {
        let got = output.pointer(pointer).cloned().unwrap_or(Value::Null);
        if &got != expected {
            r.mismatches.push(Mismatch {
                pointer: pointer.clone(),
                expected: expected.clone(),
                got,
            });
        }
    }
    if let Some(pointer) = &case.findings {
        let Some(items) = output.pointer(pointer).and_then(Value::as_array) else {
            r.error = Some(format!("the output has no array at {pointer}"));
            return;
        };
        let mut keys: Vec<String> = items.iter().map(|f| key(case.key.as_deref(), f)).collect();
        keys.sort();
        keys.dedup();
        r.flagged = keys.len();
        for k in &keys {
            if case.real.contains(k) {
                r.real_found.push(k.clone());
            } else if case.false_.contains(k) {
                r.false_found.push(k.clone());
            } else {
                r.unlabeled.push(k.clone());
            }
        }
        r.missed = case
            .real
            .iter()
            .filter(|k| !keys.contains(k))
            .cloned()
            .collect();
        let labeled = r.real_found.len() + r.false_found.len();
        r.precision = (labeled > 0).then(|| r.real_found.len() as f64 / labeled as f64);
        r.recall =
            (!case.real.is_empty()).then(|| r.real_found.len() as f64 / case.real.len() as f64);
    }
    if let (Some(min), Some(p)) = (case.min_precision, r.precision)
        && p < min
    {
        r.failures
            .push(format!("precision {:.0}% < {:.0}%", p * 100.0, min * 100.0));
    }
    if let (Some(min), Some(rc)) = (case.min_recall, r.recall)
        && rc < min
    {
        r.failures
            .push(format!("recall {:.0}% < {:.0}%", rc * 100.0, min * 100.0));
    }
    if !r.mismatches.is_empty() {
        r.failures
            .push(format!("{} expected values differ", r.mismatches.len()));
    }
    r.ok = r.error.is_none() && r.failures.is_empty();
}

/// A finding's name from `template`'s `{field}` placeholders, or its JSON.
fn key(template: Option<&str>, finding: &Value) -> String {
    let Some(t) = template else {
        return finding.to_string();
    };
    let mut out = String::new();
    let mut rest = t;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let field = &after[..close];
        match finding.get(field) {
            Some(Value::String(s)) => out.push_str(s),
            Some(Value::Null) | None => {}
            Some(v) => out.push_str(&v.to_string()),
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn case() -> Case {
        Case {
            name: "t".into(),
            repo: ".".into(),
            rev: None,
            run: vec![],
            findings: Some("/failing".into()),
            key: Some("{doc}:{line} {claim}".into()),
            real: vec!["a.md:1 ClientId".into(), "a.md:9 Gone".into()],
            false_: vec!["b.md:2 Accepted".into()],
            unlabeled: vec![],
            expect: serde_json::Map::new(),
            min_precision: Some(0.9),
            min_recall: None,
        }
    }

    #[test]
    fn keys_findings_from_their_fields() {
        let f = json!({"doc": "a.md", "line": 1, "claim": "ClientId", "kind": "symbol"});
        assert_eq!(key(Some("{doc}:{line} {claim}"), &f), "a.md:1 ClientId");
        assert_eq!(key(Some("{missing}x"), &f), "x");
        assert_eq!(key(None, &json!({"a": 1})), r#"{"a":1}"#);
    }

    #[test]
    fn scores_precision_recall_and_gates() {
        let output = json!({"failing": [
            {"doc": "a.md", "line": 1, "claim": "ClientId"},
            {"doc": "b.md", "line": 2, "claim": "Accepted"},
            {"doc": "c.md", "line": 3, "claim": "New"},
        ]});
        let mut r = run_result();
        score(&case(), &output, &mut r);
        assert_eq!(r.flagged, 3);
        assert_eq!(r.real_found, ["a.md:1 ClientId"]);
        assert_eq!(r.false_found, ["b.md:2 Accepted"]);
        assert_eq!(r.unlabeled, ["c.md:3 New"]);
        assert_eq!(r.missed, ["a.md:9 Gone"]);
        assert_eq!(r.precision, Some(0.5));
        assert_eq!(r.recall, Some(0.5));
        assert_eq!(r.failures, ["precision 50% < 90%"]);
        assert!(!r.ok);
    }

    #[test]
    fn checks_expected_values() {
        let mut c = case();
        c.findings = None;
        c.min_precision = None;
        c.expect.insert("/alternatives/0/files".into(), json!(0));
        let mut r = run_result();
        score(&c, &json!({"alternatives": [{"files": 3}]}), &mut r);
        assert_eq!(r.mismatches.len(), 1);
        assert_eq!(r.mismatches[0].got, json!(3));
        assert!(!r.ok);
    }

    fn run_result() -> CaseResult {
        CaseResult {
            name: "t".into(),
            ok: false,
            error: None,
            flagged: 0,
            real_found: vec![],
            false_found: vec![],
            missed: vec![],
            unlabeled: vec![],
            precision: None,
            recall: None,
            scored: true,
            expected: 0,
            mismatches: vec![],
            failures: vec![],
        }
    }
}
