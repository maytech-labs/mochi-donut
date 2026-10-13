//! `matcha recipe`: what else usually changes when I change this? Learned from
//! which files changed together in history (docs/design.md, "recipe").

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use mochi_index::{CoChange, Commit};
use serde::Serialize;

/// Thresholds for calling a file a companion. Defaults: at least 90% of at
/// least 6 changes, so `--check` stays quiet unless sure (ADR 0012).
#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    pub min_support: u32,
    pub min_confidence: f64,
    /// How much likelier than usual the companion must be to change.
    pub min_lift: f64,
    /// Commits touching more files than this are ignored (mass edits).
    pub max_files: usize,
}

/// A companion file that didn't change.
#[derive(Debug, Clone, Serialize)]
pub struct Missing {
    /// The changed file whose history predicts the missing one.
    pub changed: String,
    pub missing: String,
    pub together: u32,
    pub of: u32,
    /// A command that usually produces the missing file, if one was found.
    pub fix: Option<String>,
}

/// Files that usually change with something in `changed` but didn't. One
/// entry per missing file, citing the strongest evidence for it.
pub fn check(
    changed: &[String],
    cc: &CoChange,
    t: Thresholds,
    exists: impl Fn(&str) -> bool,
) -> Vec<Missing> {
    let changed_set: HashSet<&str> = changed.iter().map(String::as_str).collect();
    let mut best: BTreeMap<String, Missing> = BTreeMap::new();
    for file in changed {
        for c in cc.companions(file, t.min_support, t.min_confidence, t.min_lift) {
            if changed_set.contains(c.file.as_str()) || !exists(&c.file) {
                continue;
            }
            let candidate = Missing {
                changed: file.clone(),
                missing: c.file.clone(),
                together: c.together,
                of: c.of,
                fix: None,
            };
            let stronger = best
                .get(&c.file)
                .is_none_or(|m| (c.together, c.of) > (m.together, m.of));
            if stronger {
                best.insert(c.file, candidate);
            }
        }
    }
    let mut out: Vec<Missing> = best.into_values().collect();
    out.sort_by(|a, b| {
        b.together
            .cmp(&a.together)
            .then_with(|| a.missing.cmp(&b.missing))
    });
    out
}

/// Looks for the command that regenerates `file`: a Makefile target whose
/// recipe names it, or a `package.json` script that does, in the repo root or
/// any directory above the file.
pub fn fix_hint(root: &Path, file: &str) -> Option<String> {
    let name = file.rsplit('/').next().unwrap_or(file);
    for makefile in ["Makefile", "makefile", "GNUmakefile"] {
        let Ok(text) = std::fs::read_to_string(root.join(makefile)) else {
            continue;
        };
        if let Some(target) = make_target_naming(&text, name) {
            return Some(format!("make {target}"));
        }
    }
    let mut dir = Path::new(file).parent();
    while let Some(d) = dir {
        if let Some(script) = npm_script_naming(&root.join(d).join("package.json"), name) {
            let prefix = if d.as_os_str().is_empty() {
                String::new()
            } else {
                format!("--prefix {} ", d.display())
            };
            return Some(format!("npm {prefix}run {script}"));
        }
        dir = d.parent();
    }
    None
}

fn make_target_naming(makefile: &str, name: &str) -> Option<String> {
    let mut target: Option<&str> = None;
    for line in makefile.lines() {
        if let Some(recipe) = line.strip_prefix('\t') {
            if target.is_some() && recipe.contains(name) {
                return target.map(str::to_owned);
            }
        } else if let Some((head, _)) = line.split_once(':') {
            let head = head.trim();
            let plain = !head.is_empty()
                && !head.starts_with('.')
                && !line.contains(":=")
                && head
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c));
            target = plain.then_some(head);
        } else if !line.trim().is_empty() {
            target = None;
        }
    }
    None
}

fn npm_script_naming(package_json: &Path, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(package_json).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("scripts")?
        .as_object()?
        .iter()
        .find(|(_, cmd)| cmd.as_str().is_some_and(|c| c.contains(name)))
        .map(|(script, _)| script.clone())
}

/// Files that past changes like `query` had in common.
#[derive(Debug, Serialize)]
pub struct Like {
    pub query: String,
    pub similar: Vec<SimilarCommit>,
    /// Files and patterns most of the similar commits changed.
    pub files: Vec<FileShare>,
    /// Files only some of them changed, left out of `files`.
    pub other_files: usize,
}

#[derive(Debug, Serialize)]
pub struct SimilarCommit {
    pub hash: String,
    pub subject: String,
}

#[derive(Debug, Serialize)]
pub struct FileShare {
    /// A path, or a pattern like `routes/*.rs` when the file name varies.
    pub path: String,
    pub commits: usize,
}

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "into", "from", "add", "adds", "new", "use", "make", "this",
    "that",
];

/// Finds commits whose subjects share words with `query` (or, weighted
/// less, whose changed paths do: "migration" finds commits that touched
/// `migrations/`), then reports the files (or file patterns) most of them
/// changed.
pub fn like(query: &str, commits: &[Commit], max_similar: usize) -> Like {
    let terms: Vec<String> = subject_words(query)
        .into_iter()
        .filter(|w| !STOPWORDS.contains(&w.as_str()))
        .collect();
    let mut scored: Vec<(usize, &Commit)> = commits
        .iter()
        .map(|c| {
            (
                2 * score(&terms, &c.subject) + path_score(&terms, &c.files),
                c,
            )
        })
        .filter(|(s, _)| *s > 0)
        .collect();
    let best = scored.iter().map(|(s, _)| *s).max().unwrap_or(0);
    // Keep commits that match at least half as well as the best one.
    scored.retain(|(s, _)| *s * 2 >= best);
    scored.sort_by_key(|a| std::cmp::Reverse(a.0));
    scored.truncate(max_similar);
    let chosen: Vec<&Commit> = scored.into_iter().map(|(_, c)| c).collect();

    let n = chosen.len();
    let mut exact: BTreeMap<&str, usize> = BTreeMap::new();
    let mut patterns: BTreeMap<String, usize> = BTreeMap::new();
    for c in &chosen {
        let mut seen = HashSet::new();
        for f in &c.files {
            *exact.entry(f.as_str()).or_insert(0) += 1;
            let p = generalize(f);
            if seen.insert(p.clone()) {
                *patterns.entry(p).or_insert(0) += 1;
            }
        }
    }
    // A strict majority: with 2 similar commits, a file needs both.
    let half = n / 2 + 1;
    let other_files = exact.values().filter(|k| **k < half).count();
    let mut files: Vec<FileShare> = exact
        .iter()
        .filter(|(_, k)| **k >= half)
        .map(|(f, k)| FileShare {
            path: (*f).to_owned(),
            commits: *k,
        })
        .collect();
    for (p, k) in patterns {
        let covered_by_exact = files
            .iter()
            .filter(|f| generalize(&f.path) == p)
            .any(|f| f.commits >= k);
        if k >= half && !covered_by_exact {
            files.push(FileShare {
                path: p,
                commits: k,
            });
        }
    }
    // Three or more files in one directory with one extension and the same
    // count read better as their pattern: `.sqlx/*.json`, not 13 hashes.
    let mut groups: BTreeMap<(String, usize), usize> = BTreeMap::new();
    for f in files.iter().filter(|f| !f.path.contains('*')) {
        *groups.entry((generalize(&f.path), f.commits)).or_insert(0) += 1;
    }
    for ((pattern, k), count) in groups {
        if count >= 3 && pattern != "*" {
            files.retain(|f| {
                f.path.contains('*') || generalize(&f.path) != pattern || f.commits != k
            });
            if !files.iter().any(|f| f.path == pattern) {
                files.push(FileShare {
                    path: format!("{pattern}  ({count} files)"),
                    commits: k,
                });
            }
        }
    }
    files.sort_by(|a, b| b.commits.cmp(&a.commits).then_with(|| a.path.cmp(&b.path)));
    Like {
        query: query.to_owned(),
        similar: chosen
            .iter()
            .map(|c| SimilarCommit {
                hash: c.hash.chars().take(7).collect(),
                subject: c.subject.clone(),
            })
            .collect(),
        files,
        other_files,
    }
}

/// How many query terms name a part of a path the commit changed.
fn path_score(terms: &[String], files: &[String]) -> usize {
    terms
        .iter()
        .filter(|t| {
            files.iter().any(|f| {
                subject_words(f)
                    .iter()
                    .any(|w| w.starts_with(t.as_str()) || t.starts_with(w.as_str()))
            })
        })
        .count()
}

fn subject_words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(str::to_ascii_lowercase)
        .collect()
}

/// How many query terms appear in `subject`, allowing simple suffixes
/// ("endpoint" matches "endpoints").
fn score(terms: &[String], subject: &str) -> usize {
    let words = subject_words(subject);
    terms
        .iter()
        .filter(|t| {
            words
                .iter()
                .any(|w| w.starts_with(t.as_str()) || t.starts_with(w.as_str()))
        })
        .count()
}

/// `a/b/holders.rs` becomes `a/b/*.rs`.
fn generalize(path: &str) -> String {
    let (dir, name) = path.rsplit_once('/').map_or(("", path), |(d, n)| (d, n));
    let ext = name.rsplit_once('.').map(|(_, e)| e);
    let pattern = ext.map_or_else(|| "*".to_owned(), |e| format!("*.{e}"));
    if dir.is_empty() {
        pattern
    } else {
        format!("{dir}/{pattern}")
    }
}

/// How `check` would have done on recent commits.
#[derive(Debug, Serialize)]
pub struct Replay {
    pub commits_replayed: usize,
    /// One test per file per commit: remove the file, run `check`.
    pub removals_tested: usize,
    /// Removed files that `check` named.
    pub caught: usize,
    /// Files `check` named for the complete commit, which needed nothing
    /// more. Counted once per commit, as a user running it once would see.
    pub false_alarms: usize,
    /// Commits where the complete commit got at least one false alarm.
    pub noisy_commits: usize,
    /// caught / (caught + false_alarms)
    pub precision: Option<f64>,
    /// caught / removals_tested
    pub recall: f64,
    pub catches: Vec<Catch>,
    pub examples_of_false_alarms: Vec<Catch>,
}

#[derive(Debug, Serialize)]
pub struct Catch {
    pub commit: String,
    pub missing: String,
    pub changed: String,
    pub together: u32,
    pub of: u32,
}

/// Replays the newest `n` commits in `commits` (newest first). For each one,
/// learns only from the commits before it. Checking the complete commit gives
/// its false alarms; removing each file in turn and checking again shows
/// whether `check` would have caught it.
pub fn replay(commits: &[Commit], n: usize, t: Thresholds) -> Replay {
    let mut r = Replay {
        commits_replayed: 0,
        removals_tested: 0,
        caught: 0,
        false_alarms: 0,
        noisy_commits: 0,
        precision: None,
        recall: 0.0,
        catches: Vec::new(),
        examples_of_false_alarms: Vec::new(),
    };
    for (i, target) in commits.iter().take(n).enumerate() {
        if target.files.len() < 2 || target.files.len() > t.max_files {
            continue;
        }
        let cc = CoChange::build(&commits[i + 1..], t.max_files);
        let short: String = target.hash.chars().take(7).collect();
        r.commits_replayed += 1;

        let alarms = check(&target.files, &cc, t, |_| true);
        r.false_alarms += alarms.len();
        r.noisy_commits += usize::from(!alarms.is_empty());
        for m in alarms.into_iter().take(2) {
            r.examples_of_false_alarms.push(Catch {
                commit: short.clone(),
                missing: m.missing,
                changed: m.changed,
                together: m.together,
                of: m.of,
            });
        }

        for removed in &target.files {
            let changed: Vec<String> = target
                .files
                .iter()
                .filter(|f| *f != removed)
                .cloned()
                .collect();
            r.removals_tested += 1;
            if let Some(m) = check(&changed, &cc, t, |_| true)
                .into_iter()
                .find(|m| &m.missing == removed)
            {
                r.caught += 1;
                r.catches.push(Catch {
                    commit: short.clone(),
                    missing: m.missing,
                    changed: m.changed,
                    together: m.together,
                    of: m.of,
                });
            }
        }
    }
    if r.caught + r.false_alarms > 0 {
        r.precision = Some(r.caught as f64 / (r.caught + r.false_alarms) as f64);
    }
    if r.removals_tested > 0 {
        r.recall = r.caught as f64 / r.removals_tested as f64;
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Thresholds = Thresholds {
        min_support: 3,
        min_confidence: 0.9,
        min_lift: 0.0,
        max_files: 100,
    };

    fn commit(hash: &str, subject: &str, files: &[&str]) -> Commit {
        Commit {
            hash: hash.into(),
            subject: subject.into(),
            files: files.iter().map(|f| (*f).to_owned()).collect(),
        }
    }

    fn history() -> Vec<Commit> {
        vec![
            commit(
                "5",
                "feat: add invoices endpoint",
                &[
                    "api/openapi.rs",
                    "web/openapi.json",
                    "api/routes/invoices.rs",
                ],
            ),
            commit(
                "4",
                "feat: add holders endpoint",
                &[
                    "api/openapi.rs",
                    "web/openapi.json",
                    "api/routes/holders.rs",
                ],
            ),
            commit(
                "3",
                "feat: add clients endpoint",
                &[
                    "api/openapi.rs",
                    "web/openapi.json",
                    "api/routes/clients.rs",
                ],
            ),
            commit("2", "docs: readme", &["README.md"]),
            commit("1", "fix: typo", &["api/routes/clients.rs"]),
        ]
    }

    #[test]
    fn names_a_missing_companion() {
        let cc = CoChange::build(&history(), 100);
        let found = check(&["api/openapi.rs".into()], &cc, T, |_| true);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].missing, "web/openapi.json");
        assert_eq!((found[0].together, found[0].of), (3, 3));
    }

    #[test]
    fn quiet_when_the_companion_changed_or_history_is_thin() {
        let cc = CoChange::build(&history(), 100);
        let both = ["api/openapi.rs".to_owned(), "web/openapi.json".to_owned()];
        assert!(check(&both, &cc, T, |_| true).is_empty());
        let strict = Thresholds {
            min_support: 4,
            ..T
        };
        assert!(check(&["api/openapi.rs".into()], &cc, strict, |_| true).is_empty());
    }

    #[test]
    fn skips_companions_that_no_longer_exist() {
        let cc = CoChange::build(&history(), 100);
        assert!(
            check(&["api/openapi.rs".into()], &cc, T, |f| f
                != "web/openapi.json")
            .is_empty()
        );
    }

    #[test]
    fn finds_the_make_target_that_writes_a_file() {
        let mk = "openapi: ## Regenerate\n\tcargo run -- openapi > frontend/src/api/openapi.json\n\nfmt:\n\tcargo fmt\n";
        assert_eq!(
            make_target_naming(mk, "openapi.json").as_deref(),
            Some("openapi")
        );
        assert_eq!(make_target_naming(mk, "nothing.txt"), None);
        let assigns = "OUT := openapi.json\nbuild:\n\techo hi\n";
        assert_eq!(make_target_naming(assigns, "openapi.json"), None);
    }

    #[test]
    fn like_reports_shared_files_and_patterns() {
        let l = like("add an endpoint", &history(), 10);
        assert_eq!(l.similar.len(), 3);
        let paths: Vec<&str> = l.files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"api/openapi.rs"));
        assert!(paths.contains(&"web/openapi.json"));
        assert!(paths.contains(&"api/routes/*.rs"));
        assert!(!paths.contains(&"api/routes/clients.rs"));
    }

    #[test]
    fn like_also_matches_changed_paths() {
        let commits = vec![
            commit(
                "1",
                "Ledger core",
                &["db/migrations/1.sql", "db/.sqlx/a.json", "src/ledger.rs"],
            ),
            commit(
                "2",
                "Renewals",
                &["db/migrations/2.sql", "db/.sqlx/b.json", "src/renewals.rs"],
            ),
            commit("3", "Tidy readme", &["README.md"]),
        ];
        let l = like("migration", &commits, 10);
        assert_eq!(l.similar.len(), 2);
        let paths: Vec<&str> = l.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            ["db/.sqlx/*.json", "db/migrations/*.sql", "src/*.rs"]
        );
        // Each exact file was in only one of the two.
        assert_eq!(l.other_files, 6);
    }

    #[test]
    fn like_collapses_many_files_into_their_pattern() {
        let commits = vec![commit(
            "1",
            "Email outbox",
            &[
                "db/.sqlx/q1.json",
                "db/.sqlx/q2.json",
                "db/.sqlx/q3.json",
                "src/mail.rs",
            ],
        )];
        let l = like("email", &commits, 10);
        let paths: Vec<&str> = l.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["db/.sqlx/*.json  (3 files)", "src/mail.rs"]);
    }

    #[test]
    fn generalizes_varying_file_names() {
        assert_eq!(generalize("a/b/holders.rs"), "a/b/*.rs");
        assert_eq!(generalize("Makefile"), "*");
    }

    #[test]
    fn replay_counts_catches_and_false_alarms() {
        let mut commits = history();
        commits.insert(
            0,
            commit(
                "6",
                "feat: add payments endpoint",
                &[
                    "api/openapi.rs",
                    "web/openapi.json",
                    "api/routes/payments.rs",
                ],
            ),
        );
        let r = replay(&commits, 1, T);
        assert_eq!(r.commits_replayed, 1);
        assert_eq!(r.removals_tested, 3);
        // Removing either openapi file is caught; the new route can't be predicted.
        assert_eq!(r.caught, 2);
        assert_eq!(r.false_alarms, 0);
        assert_eq!(r.precision, Some(1.0));
    }
}
