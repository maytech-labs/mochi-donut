//! `taro ready`: can this PR merge, and if not, what's blocking it? Checks,
//! reviews, open threads, conflicts, and the repo's own conventions as its
//! merged PRs show them (docs/taro/design.md, "taro ready").

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Ok,
    /// Worth fixing, but the forge would still let it merge.
    Warn,
    /// Blocks the merge.
    Block,
    /// Couldn't be told.
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

fn item(name: &'static str, state: State, detail: impl Into<String>, fix: Option<String>) -> Item {
    Item {
        name,
        state,
        detail: detail.into(),
        fix,
    }
}

/// Everything `ready` judges, gathered from the forge.
pub struct Facts<'a> {
    /// `gh pr view --json` output.
    pub pr: &'a Value,
    /// Required check names, when the forge would say.
    pub required: Option<Vec<String>>,
    /// Unresolved review threads.
    pub unresolved: Option<usize>,
    /// The last merged PRs: their titles and changed files.
    pub merged: &'a [MergedPr],
}

pub struct MergedPr {
    pub title: String,
    pub files: Vec<String>,
}

pub fn ready(f: &Facts) -> Vec<Item> {
    let pr = f.pr;
    let s = |k: &str| pr[k].as_str().unwrap_or("").to_owned();
    let number = pr["number"].as_u64().unwrap_or(0);
    let mut items = Vec::new();

    let state = s("state");
    if state != "OPEN" {
        items.push(item(
            "state",
            State::Block,
            format!("PR #{number} is {}", state.to_lowercase()),
            None,
        ));
        return items;
    }
    if pr["isDraft"].as_bool() == Some(true) {
        items.push(item(
            "draft",
            State::Block,
            "It's a draft.",
            Some(format!("gh pr ready {number}")),
        ));
    }

    items.push(match s("mergeable").as_str() {
        "CONFLICTING" => item(
            "conflicts",
            State::Block,
            format!("It conflicts with {}.", s("baseRefName")),
            Some(format!(
                "Rebase or merge {} into {} and resolve them",
                s("baseRefName"),
                s("headRefName")
            )),
        ),
        "MERGEABLE" => item("conflicts", State::Ok, "No conflicts.", None),
        _ => item(
            "conflicts",
            State::Unknown,
            "The forge hasn't worked out mergeability yet.",
            None,
        ),
    });
    if s("mergeStateStatus") == "BEHIND" {
        items.push(item(
            "behind",
            State::Warn,
            format!("{} has moved on since it branched.", s("baseRefName")),
            Some(format!(
                "Update {} from {}",
                s("headRefName"),
                s("baseRefName")
            )),
        ));
    }

    items.push(checks_item(
        pr["statusCheckRollup"]
            .as_array()
            .map_or(&[][..], Vec::as_slice),
        f.required.as_deref(),
    ));

    items.push(match s("reviewDecision").as_str() {
        "CHANGES_REQUESTED" => item(
            "reviews",
            State::Block,
            "A reviewer requested changes.",
            Some(format!("taro reviews {number}")),
        ),
        "REVIEW_REQUIRED" => item(
            "reviews",
            State::Block,
            "It needs an approving review.",
            Some("taro owners --diff names who to ask".into()),
        ),
        "APPROVED" => item("reviews", State::Ok, "Approved.", None),
        _ => item("reviews", State::Ok, "No review is required.", None),
    });

    items.push(match f.unresolved {
        Some(0) => item("threads", State::Ok, "No unresolved review threads.", None),
        Some(n) => item(
            "threads",
            State::Block,
            format!(
                "{n} unresolved review {}.",
                if n == 1 { "thread" } else { "threads" }
            ),
            Some(format!("taro reviews {number}")),
        ),
        None => item(
            "threads",
            State::Unknown,
            "Review threads couldn't be read.",
            None,
        ),
    });

    let files: Vec<String> = pr["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x["path"].as_str().map(str::to_owned))
        .collect();
    items.extend(conventions(&s("title"), &files, f.merged));
    items
}

fn checks_item(rollup: &[Value], required: Option<&[String]>) -> Item {
    let mut failed = Vec::new();
    let mut pending = Vec::new();
    let mut names = Vec::new();
    for c in rollup {
        let name = c["name"]
            .as_str()
            .or_else(|| c["context"].as_str())
            .unwrap_or("?")
            .to_owned();
        let outcome = if c["__typename"] == "StatusContext" {
            match c["state"].as_str().unwrap_or("") {
                "SUCCESS" => "pass",
                "FAILURE" | "ERROR" => "fail",
                _ => "pending",
            }
        } else if c["status"].as_str() != Some("COMPLETED") {
            "pending"
        } else {
            match c["conclusion"].as_str().unwrap_or("") {
                "SUCCESS" | "NEUTRAL" | "SKIPPED" => "pass",
                _ => "fail",
            }
        };
        let url = c["detailsUrl"]
            .as_str()
            .or_else(|| c["targetUrl"].as_str())
            .unwrap_or("");
        match outcome {
            "fail" => failed.push((name.clone(), url.to_owned())),
            "pending" => pending.push(name.clone()),
            _ => {}
        }
        names.push(name);
    }
    let missing: Vec<String> = required
        .unwrap_or(&[])
        .iter()
        .filter(|r| !names.contains(r))
        .cloned()
        .collect();
    let counts = |n: &str| required.is_none_or(|r| r.iter().any(|x| x == n));
    let blocking_failed: Vec<&(String, String)> =
        failed.iter().filter(|(n, _)| counts(n)).collect();
    let blocking_pending: Vec<&String> = pending.iter().filter(|n| counts(n)).collect();
    let scope = if required.is_some() {
        ""
    } else {
        " The required checks couldn't be read (the forge's rules API needs admin rights or a paid plan), so every check counts."
    };
    if !blocking_failed.is_empty() {
        let list: Vec<String> = blocking_failed
            .iter()
            .map(|(n, u)| {
                if u.is_empty() {
                    n.clone()
                } else {
                    format!("{n} ({u})")
                }
            })
            .collect();
        return item("checks", State::Block, format!("Failed: {}.{scope}", list.join(", ")), Some("taro flaky shows whether a failure is known to be flaky; ube run reproduces it locally".into()));
    }
    if !blocking_pending.is_empty() || !missing.is_empty() {
        let mut parts = Vec::new();
        if !blocking_pending.is_empty() {
            parts.push(format!(
                "still running: {}",
                blocking_pending
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !missing.is_empty() {
            parts.push(format!("required but not reported: {}", missing.join(", ")));
        }
        return item(
            "checks",
            State::Block,
            format!("Checks {}.{scope}", parts.join("; ")),
            Some("yuzu wait ci <pr> waits for them".into()),
        );
    }
    if !failed.is_empty() {
        return item(
            "checks",
            State::Warn,
            format!(
                "Optional checks failed: {}.",
                failed
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            None,
        );
    }
    if names.is_empty() {
        return item("checks", State::Ok, "No checks are reported.", None);
    }
    item(
        "checks",
        State::Ok,
        format!("All {} checks passed.{scope}", names.len()),
        None,
    )
}

/// How far a convention must hold in merged PRs before it blocks or warns,
/// and how many merged PRs it takes to say.
const MIN_PRS: usize = 10;
const BLOCK_SHARE: f64 = 0.9;
const WARN_SHARE: f64 = 0.6;

/// The repo's conventions as its last merged PRs show them: a changelog
/// entry, an ADR, Conventional Commits titles.
pub fn conventions(title: &str, files: &[String], merged: &[MergedPr]) -> Vec<Item> {
    if merged.len() < MIN_PRS {
        return vec![item(
            "conventions",
            State::Unknown,
            format!(
                "Only {} merged PRs to learn conventions from; it takes {MIN_PRS}.",
                merged.len()
            ),
            None,
        )];
    }
    let cc = Regex::new(
        r"^(feat|fix|docs|test|refactor|perf|build|ci|chore|style|revert)(\([^)]+\))?!?: \S",
    )
    .ok();
    let is_cc = |t: &str| cc.as_ref().is_some_and(|re| re.is_match(t));
    let changelog = |f: &str| {
        f.rsplit('/')
            .next()
            .unwrap_or(f)
            .to_ascii_uppercase()
            .starts_with("CHANGELOG")
    };
    let adr = |f: &str| {
        f.split('/')
            .any(|s| matches!(s, "decisions" | "adr" | "adrs"))
            && f.ends_with(".md")
    };
    let mut out = Vec::new();
    let mut judge = |name: &'static str, what: &str, share_of: usize, has: bool, fix: &str| {
        let share = share_of as f64 / merged.len() as f64;
        let evidence = format!("{share_of} of the last {} merged PRs {what}", merged.len());
        let state = if has || share < WARN_SHARE {
            State::Ok
        } else if share >= BLOCK_SHARE {
            State::Block
        } else {
            State::Warn
        };
        let detail = if has {
            format!("{evidence}; this one does too.")
        } else if share < WARN_SHARE {
            format!("{evidence}, so it isn't expected.")
        } else {
            format!("{evidence}; this one doesn't.")
        };
        out.push(item(
            name,
            state,
            detail,
            (state != State::Ok).then(|| fix.to_owned()),
        ));
    };
    judge(
        "changelog",
        "changed a changelog",
        merged
            .iter()
            .filter(|p| p.files.iter().any(|f| changelog(f)))
            .count(),
        files.iter().any(|f| changelog(f)),
        "Add an entry to the changelog",
    );
    judge(
        "adr",
        "added or changed an ADR",
        merged
            .iter()
            .filter(|p| p.files.iter().any(|f| adr(f)))
            .count(),
        files.iter().any(|f| adr(f)),
        "Record the decision as an ADR, or say why there's none",
    );
    judge(
        "title",
        "had a Conventional Commits title",
        merged.iter().filter(|p| is_cc(&p.title)).count(),
        is_cc(title),
        "Retitle it as type(scope): summary",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pr(extra: Value) -> Value {
        let mut base = json!({
            "number": 7, "state": "OPEN", "isDraft": false, "mergeable": "MERGEABLE",
            "mergeStateStatus": "CLEAN", "reviewDecision": "", "title": "feat: x",
            "baseRefName": "main", "headRefName": "feat/x",
            "statusCheckRollup": [
                {"__typename": "CheckRun", "name": "linux", "status": "COMPLETED", "conclusion": "SUCCESS", "detailsUrl": "u1"},
                {"__typename": "StatusContext", "context": "ci/legacy", "state": "SUCCESS", "targetUrl": "u2"}
            ],
            "files": [{"path": "src/a.rs"}]
        });
        if let (Some(b), Some(e)) = (base.as_object_mut(), extra.as_object()) {
            b.extend(e.clone());
        }
        base
    }

    fn states(items: &[Item]) -> Vec<(&str, State)> {
        items.iter().map(|i| (i.name, i.state)).collect()
    }

    #[test]
    fn a_clean_pr_is_ready() {
        let p = pr(json!({}));
        let items = ready(&Facts {
            pr: &p,
            required: None,
            unresolved: Some(0),
            merged: &[],
        });
        assert!(items.iter().all(|i| i.state != State::Block), "{items:?}");
    }

    #[test]
    fn each_blocker_says_what_to_do() {
        let p = pr(json!({
            "isDraft": true, "mergeable": "CONFLICTING", "reviewDecision": "CHANGES_REQUESTED",
            "statusCheckRollup": [{"__typename": "CheckRun", "name": "linux", "status": "COMPLETED", "conclusion": "FAILURE", "detailsUrl": "https://ci/1"}]
        }));
        let items = ready(&Facts {
            pr: &p,
            required: None,
            unresolved: Some(2),
            merged: &[],
        });
        let blocked: Vec<&str> = items
            .iter()
            .filter(|i| i.state == State::Block)
            .map(|i| i.name)
            .collect();
        assert_eq!(
            blocked,
            ["draft", "conflicts", "checks", "reviews", "threads"]
        );
        assert!(
            items
                .iter()
                .filter(|i| i.state == State::Block)
                .all(|i| i.fix.is_some())
        );
        assert!(items[2].detail.contains("linux (https://ci/1)"));
    }

    #[test]
    fn only_required_checks_block_when_known() {
        let rollup = [
            json!({"__typename": "CheckRun", "name": "linux", "status": "COMPLETED", "conclusion": "SUCCESS"}),
            json!({"__typename": "CheckRun", "name": "lint", "status": "COMPLETED", "conclusion": "FAILURE"}),
            json!({"__typename": "CheckRun", "name": "macos", "status": "IN_PROGRESS", "conclusion": ""}),
        ];
        let req = vec!["linux".to_owned()];
        assert_eq!(checks_item(&rollup, Some(&req)).state, State::Warn);
        let req = vec!["linux".to_owned(), "macos".to_owned(), "deploy".to_owned()];
        let i = checks_item(&rollup, Some(&req));
        assert_eq!(i.state, State::Block);
        assert!(i.detail.contains("still running: macos"));
        assert!(i.detail.contains("required but not reported: deploy"));
        let i = checks_item(&rollup, None);
        assert_eq!(i.state, State::Block);
        assert!(i.detail.contains("every check counts"));
    }

    /// `n` merged PRs, the first `with_changelog` of them changing the changelog.
    fn merged(n: usize, with_changelog: usize, title: &str) -> Vec<MergedPr> {
        (0..n)
            .map(|i| MergedPr {
                title: title.into(),
                files: if i < with_changelog {
                    vec!["CHANGELOG.md".into(), "src/a.rs".into()]
                } else {
                    vec!["src/a.rs".into()]
                },
            })
            .collect()
    }

    #[test]
    fn conventions_come_from_merged_prs_with_their_counts() {
        let files = vec!["src/a.rs".to_owned()];
        let few = conventions("feat: x", &files, &merged(4, 4, "feat: y"));
        assert_eq!(few[0].state, State::Unknown);
        let items = conventions("Add a thing", &files, &merged(10, 10, "feat: y"));
        assert_eq!(
            states(&items),
            [
                ("changelog", State::Block),
                ("adr", State::Ok),
                ("title", State::Block)
            ]
        );
        assert_eq!(
            items[0].detail,
            "10 of the last 10 merged PRs changed a changelog; this one doesn't."
        );
        let items = conventions("feat: x", &files, &merged(10, 7, "feat: y"));
        assert_eq!(items[0].state, State::Warn);
        assert_eq!(
            conventions("feat: x", &files, &merged(10, 5, "feat: y"))[0].state,
            State::Ok
        );
        assert_eq!(items[2].state, State::Ok);
    }
}
