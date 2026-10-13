//! `taro flaky`: which checks fail and then pass on the same commit. From
//! GitHub Actions runs in a window: a run that passed on a rerun after a
//! failed attempt, or a workflow that both failed and passed on one commit
//! (docs/taro/design.md, "taro flaky").

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

/// One workflow run, as the runs API lists it.
#[derive(Debug, Clone)]
pub struct Run {
    pub id: u64,
    pub workflow: String,
    pub sha: String,
    pub conclusion: String,
    pub attempt: u64,
    pub created: String,
    pub url: String,
}

pub fn runs(answer: &Value) -> Vec<Run> {
    answer["workflow_runs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| Run {
            id: r["id"].as_u64().unwrap_or(0),
            workflow: r["name"].as_str().unwrap_or("?").to_owned(),
            sha: r["head_sha"].as_str().unwrap_or("").to_owned(),
            conclusion: r["conclusion"].as_str().unwrap_or("").to_owned(),
            attempt: r["run_attempt"].as_u64().unwrap_or(1),
            created: r["created_at"].as_str().unwrap_or("").to_owned(),
            url: r["html_url"].as_str().unwrap_or("").to_owned(),
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub struct Flaky {
    /// The workflow, and the job when it's known.
    pub check: String,
    /// Commits where it failed and then passed.
    pub flakes: usize,
    /// Completed runs of the workflow in the window.
    pub runs: usize,
    pub rate: f64,
    pub last: String,
    pub example: String,
}

/// Records a flake of `check` on `r`'s commit, keeping the latest run.
fn note(events: &mut BTreeMap<(String, String), (String, String)>, check: String, r: &Run) {
    let e = events
        .entry((check, r.sha.clone()))
        .or_insert((String::new(), String::new()));
    if r.created > e.0 {
        *e = (r.created.clone(), r.url.clone());
    }
}

/// `jobs_failed_then_passed` maps a rerun's run id to the job names that
/// failed in an earlier attempt and passed in the last.
pub fn flaky(runs: &[Run], jobs_failed_then_passed: &BTreeMap<u64, Vec<String>>) -> Vec<Flaky> {
    let passed = |c: &str| matches!(c, "success" | "skipped" | "neutral");
    let failed = |c: &str| matches!(c, "failure" | "timed_out" | "cancelled");
    let mut totals: BTreeMap<&str, usize> = BTreeMap::new();
    for r in runs.iter().filter(|r| !r.conclusion.is_empty()) {
        *totals.entry(r.workflow.as_str()).or_insert(0) += 1;
    }
    // (check, sha) -> (latest time, example url)
    let mut events: BTreeMap<(String, String), (String, String)> = BTreeMap::new();
    // A rerun that passed after a failed attempt.
    for r in runs
        .iter()
        .filter(|r| r.attempt > 1 && passed(&r.conclusion))
    {
        match jobs_failed_then_passed.get(&r.id) {
            Some(jobs) if !jobs.is_empty() => jobs
                .iter()
                .for_each(|j| note(&mut events, format!("{} / {j}", r.workflow), r)),
            _ => note(&mut events, r.workflow.clone(), r),
        }
    }
    // The same workflow failing and passing on one commit.
    let mut by_sha: BTreeMap<(&str, &str), (bool, bool, &Run)> = BTreeMap::new();
    for r in runs {
        let e = by_sha
            .entry((r.workflow.as_str(), r.sha.as_str()))
            .or_insert((false, false, r));
        e.0 |= failed(&r.conclusion);
        e.1 |= passed(&r.conclusion);
        if r.created > e.2.created {
            e.2 = r;
        }
    }
    for ((wf, _), (f, p, r)) in &by_sha {
        let already = events
            .keys()
            .any(|(c, s)| (c == wf || c.starts_with(&format!("{wf} / "))) && *s == r.sha);
        if *f && *p && !already {
            note(&mut events, (*wf).to_owned(), r);
        }
    }
    let mut out: BTreeMap<String, Flaky> = BTreeMap::new();
    for ((check, _), (at, url)) in events {
        let wf = check.split(" / ").next().unwrap_or(&check).to_owned();
        let total = totals.get(wf.as_str()).copied().unwrap_or(0).max(1);
        let f = out.entry(check.clone()).or_insert(Flaky {
            check,
            flakes: 0,
            runs: total,
            rate: 0.0,
            last: String::new(),
            example: String::new(),
        });
        f.flakes += 1;
        if at > f.last {
            f.last = at;
            f.example = url;
        }
    }
    let mut v: Vec<Flaky> = out.into_values().collect();
    for f in &mut v {
        f.rate = f.flakes as f64 / f.runs as f64;
    }
    v.sort_by(|a, b| b.flakes.cmp(&a.flakes).then_with(|| a.check.cmp(&b.check)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: u64, wf: &str, sha: &str, conclusion: &str, attempt: u64) -> Run {
        Run {
            id,
            workflow: wf.into(),
            sha: sha.into(),
            conclusion: conclusion.into(),
            attempt,
            created: format!("2026-10-0{id}T00:00:00Z"),
            url: format!("https://ci/{id}"),
        }
    }

    #[test]
    fn reruns_and_mixed_outcomes_are_flakes_and_real_failures_are_not() {
        let runs = [
            run(1, "CI", "a", "success", 2),
            run(2, "CI", "b", "failure", 1),
            run(3, "CI", "b", "success", 1),
            run(4, "CI", "c", "failure", 1),
            run(5, "Deploy", "c", "success", 1),
        ];
        let jobs: BTreeMap<u64, Vec<String>> = [(1, vec!["check (macos-15)".to_owned()])].into();
        let f = flaky(&runs, &jobs);
        let got: Vec<(&str, usize, usize)> = f
            .iter()
            .map(|x| (x.check.as_str(), x.flakes, x.runs))
            .collect();
        assert_eq!(got, [("CI", 1, 4), ("CI / check (macos-15)", 1, 4)]);
        // Commit c failed for real: it never passed on CI.
        assert!(f.iter().all(|x| x.example != "https://ci/4"));
    }
}
