//! The local index of a repo's files and history that matcha's commands read
//! (docs/matcha/design.md, "The index"). It asks `git` directly rather than linking a
//! git library, and builds what it needs in memory on each run
//! (docs/decisions/0007-phase-1-index.md).

mod cochange;
mod git;

use std::path::{Path, PathBuf};

pub use cochange::{CoChange, Companion};

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("{0} isn't inside a git repository")]
    NotARepo(PathBuf),
    #[error("couldn't read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("couldn't run git: {0}. Is git installed and on PATH?")]
    GitMissing(std::io::Error),
    #[error("`git {args}` failed: {stderr}")]
    Git { args: String, stderr: String },
}

/// The root of a git repository: the directory that holds `.git`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRoot(PathBuf);

impl RepoRoot {
    /// Walks up from `start` to the nearest directory containing `.git`
    /// (a directory, or a file for worktrees and submodules).
    pub fn discover(start: &Path) -> Result<RepoRoot, IndexError> {
        let start = start.canonicalize().map_err(|source| IndexError::Io {
            path: start.to_path_buf(),
            source,
        })?;
        start
            .ancestors()
            .find(|dir| dir.join(".git").exists())
            .map(|dir| RepoRoot(dir.to_path_buf()))
            .ok_or(IndexError::NotARepo(start))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Every file git tracks, as repo-relative paths with `/` separators.
    pub fn tracked_files(&self) -> Result<Vec<String>, IndexError> {
        let out = git::run(self.path(), &["ls-files", "-z"])?;
        Ok(split_nul(&out))
    }

    /// Files changed in the working tree relative to `HEAD`, staged or not,
    /// plus untracked files that aren't ignored.
    pub fn working_changes(&self) -> Result<Vec<String>, IndexError> {
        let mut files = if git::has_head(self.path()) {
            split_nul(&git::run(
                self.path(),
                &["diff", "--name-only", "--no-renames", "-z", "HEAD"],
            )?)
        } else {
            self.tracked_files()?
        };
        files.extend(split_nul(&git::run(
            self.path(),
            &["ls-files", "--others", "--exclude-standard", "-z"],
        )?));
        files.sort();
        files.dedup();
        Ok(files)
    }

    /// The files one commit changed.
    pub fn commit_files(&self, rev: &str) -> Result<Vec<String>, IndexError> {
        let out = git::run(
            self.path(),
            &[
                "show",
                "--no-renames",
                "--name-only",
                "--format=",
                "-z",
                rev,
            ],
        )?;
        Ok(split_nul(&out))
    }

    /// Which of `paths` (repo-relative) git ignores, such as `.env` or `target/`.
    pub fn ignored(&self, paths: &[&str]) -> Result<Vec<String>, IndexError> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let mut args = vec!["check-ignore", "--"];
        args.extend_from_slice(paths);
        // Exit 1 means none of them are ignored.
        let out = git::run_ok(self.path(), &args, &[0, 1])?;
        Ok(out.lines().map(str::to_owned).collect())
    }

    /// The checked-out branch's name, or `None` when `HEAD` is detached.
    pub fn current_branch(&self) -> Result<Option<String>, IndexError> {
        let out = git::run_ok(
            self.path(),
            &["symbolic-ref", "--short", "-q", "HEAD"],
            &[0, 1],
        )?;
        let name = out.trim();
        Ok((!name.is_empty()).then(|| name.to_owned()))
    }

    /// Files the working tree changes relative to `base` (default `HEAD`),
    /// staged or not, plus untracked files that aren't ignored. Deleted
    /// files are included: a change that removes a file still needs checks.
    pub fn changed_files(&self, base: Option<&str>) -> Result<Vec<String>, IndexError> {
        let Some(base) = base else {
            return self.working_changes();
        };
        let mut files = split_nul(&git::run(
            self.path(),
            &["diff", "--name-only", "--no-renames", "-z", base, "--"],
        )?);
        files.extend(split_nul(&git::run(
            self.path(),
            &["ls-files", "--others", "--exclude-standard", "-z"],
        )?));
        files.sort();
        files.dedup();
        Ok(files)
    }

    /// The id of a git tree holding the working tree exactly as it is,
    /// tracked changes and untracked files included. Written through a
    /// temporary index, so the real one isn't touched. Two calls with the
    /// same files on disk give the same id.
    pub fn worktree_tree(&self) -> Result<String, IndexError> {
        let git_dir = git::run(self.path(), &["rev-parse", "--absolute-git-dir"])?;
        let tmp = PathBuf::from(git_dir.trim()).join(format!("ube-index-{}", std::process::id()));
        let env = [("GIT_INDEX_FILE", tmp.as_path())];
        let result = (|| {
            if git::has_head(self.path()) {
                git::run_env(self.path(), &["read-tree", "HEAD"], &env)?;
            }
            git::run_env(self.path(), &["add", "-A", "--", "."], &env)?;
            git::run_env(self.path(), &["write-tree"], &env)
        })();
        let _ = std::fs::remove_file(&tmp);
        Ok(result?.trim().to_owned())
    }

    /// Lines the working tree adds relative to `base` (default `HEAD`),
    /// staged or not, plus every line of untracked files that aren't ignored.
    pub fn added_lines(&self, base: Option<&str>) -> Result<Vec<AddedLine>, IndexError> {
        // The empty tree, for a repo with no commits yet.
        const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
        let base = match base {
            Some(b) => b,
            None if git::has_head(self.path()) => "HEAD",
            None => EMPTY_TREE,
        };
        let diff = git::run(
            self.path(),
            &[
                "diff",
                "--unified=0",
                "--no-color",
                "--no-ext-diff",
                "--no-renames",
                base,
                "--",
            ],
        )?;
        let mut out = parse_added(&diff);
        let untracked = split_nul(&git::run(
            self.path(),
            &["ls-files", "--others", "--exclude-standard", "-z"],
        )?);
        for path in untracked {
            let Ok(text) = std::fs::read_to_string(self.path().join(&path)) else {
                continue; // Binary or unreadable: no lines to check.
            };
            out.extend(text.lines().enumerate().map(|(i, l)| AddedLine {
                path: path.clone(),
                line: i + 1,
                text: l.to_owned(),
            }));
        }
        Ok(out)
    }

    /// Up to `limit` non-merge commits reachable from `rev` (default `HEAD`),
    /// newest first. Empty for a repo with no commits.
    pub fn history(&self, rev: Option<&str>, limit: usize) -> Result<Vec<Commit>, IndexError> {
        if rev.is_none() && !git::has_head(self.path()) {
            return Ok(Vec::new());
        }
        let limit = format!("--max-count={limit}");
        let mut args = vec![
            "log",
            "--no-merges",
            "--no-renames",
            "--name-only",
            // Record separator before each commit, unit separator between fields.
            "--format=%x1e%H%x1f%s",
            limit.as_str(),
        ];
        if let Some(rev) = rev {
            args.push(rev);
        }
        let out = git::run(self.path(), &args)?;
        Ok(parse_log(&out))
    }
}

/// One commit: its hash, subject line, and the files it changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub subject: String,
    pub files: Vec<String>,
}

/// A line a change adds, with where it lands in the new file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedLine {
    pub path: String,
    /// 1-based, in the new file.
    pub line: usize,
    pub text: String,
}

/// Added lines from `git diff --unified=0` output. Hunk headers say how
/// many lines follow, so a content line like `+++ x` isn't read as a header.
fn parse_added(diff: &str) -> Vec<AddedLine> {
    let mut out = Vec::new();
    let mut path: Option<String> = None;
    let (mut old_left, mut new_left, mut next) = (0usize, 0usize, 0usize);
    for line in diff.lines() {
        if old_left > 0 || new_left > 0 {
            if let Some(text) = line.strip_prefix('+') {
                if let Some(p) = &path {
                    out.push(AddedLine {
                        path: p.clone(),
                        line: next,
                        text: text.to_owned(),
                    });
                }
                next += 1;
                new_left = new_left.saturating_sub(1);
            } else if line.starts_with('-') {
                old_left = old_left.saturating_sub(1);
            }
            continue;
        }
        if let Some(p) = line.strip_prefix("+++ ") {
            path = p.strip_prefix("b/").map(str::to_owned);
        } else if let Some(hunk) = line.strip_prefix("@@ ") {
            // @@ -a,b +c,d @@ (a count of 1 may be left out)
            let range = |sign: char| -> (usize, usize) {
                let Some(r) = hunk.split_whitespace().find_map(|w| w.strip_prefix(sign)) else {
                    return (0, 0);
                };
                let mut parts = r.split(',');
                let start = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                let count = parts
                    .next()
                    .map_or(Some(1), |n| n.parse().ok())
                    .unwrap_or(0);
                (start, count)
            };
            old_left = range('-').1;
            (next, new_left) = range('+');
        }
    }
    out
}

fn split_nul(out: &str) -> Vec<String> {
    out.split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_log(out: &str) -> Vec<Commit> {
    out.split('\x1e')
        .filter_map(|record| {
            let mut lines = record.lines();
            let (hash, subject) = lines.next()?.split_once('\x1f')?;
            let files = lines
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect();
            Some(Commit {
                hash: hash.to_owned(),
                subject: subject.to_owned(),
                files,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_root_from_a_nested_directory() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir(repo.path().join(".git")).unwrap();
        let nested = repo.path().join("crates/a/src");
        std::fs::create_dir_all(&nested).unwrap();

        let root = RepoRoot::discover(&nested).unwrap();
        assert_eq!(root.path(), repo.path().canonicalize().unwrap());
    }

    #[test]
    fn a_git_file_counts_for_worktrees() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::write(repo.path().join(".git"), "gitdir: ../elsewhere").unwrap();
        assert!(RepoRoot::discover(repo.path()).is_ok());
    }

    #[test]
    fn outside_a_repo_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            RepoRoot::discover(dir.path()),
            Err(IndexError::NotARepo(_))
        ));
    }

    #[test]
    fn reports_ignored_paths() {
        let repo = tempfile::tempdir().unwrap();
        let ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success();
        assert!(ok);
        std::fs::write(repo.path().join(".gitignore"), ".env\ntarget/\n").unwrap();
        let root = RepoRoot::discover(repo.path()).unwrap();
        assert_eq!(
            root.ignored(&[".env", "src/a.rs", "target/"]).unwrap(),
            [".env", "target/"]
        );
        assert!(root.ignored(&["src/a.rs"]).unwrap().is_empty());
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn worktree_tree_tracks_content_not_the_index() {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "-q"]);
        std::fs::write(repo.path().join("a.txt"), "one\n").unwrap();
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-q", "-m", "a"]);
        let root = RepoRoot::discover(repo.path()).unwrap();
        let clean = root.worktree_tree().unwrap();
        assert_eq!(clean, root.worktree_tree().unwrap());

        std::fs::write(repo.path().join("b.txt"), "new\n").unwrap();
        let dirty = root.worktree_tree().unwrap();
        assert_ne!(clean, dirty);
        // The real index is untouched: b.txt is still untracked.
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(["status", "--porcelain"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&status.stdout), "?? b.txt\n");

        std::fs::remove_file(repo.path().join("b.txt")).unwrap();
        assert_eq!(root.worktree_tree().unwrap(), clean);
        assert_eq!(
            root.changed_files(Some("HEAD")).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn parses_added_lines() {
        let diff = "diff --git a/d.md b/d.md\n--- a/d.md\n+++ b/d.md\n@@ -3,0 +4,2 @@ intro\n+new one\n+new two\n@@ -9 +11 @@\n-old\n+changed\n@@ -20,0 +12 @@\n+++ plus\ndiff --git a/gone.md b/gone.md\n--- a/gone.md\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n";
        let lines = parse_added(diff);
        let got: Vec<(&str, usize, &str)> = lines
            .iter()
            .map(|l| (l.path.as_str(), l.line, l.text.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("d.md", 4, "new one"),
                ("d.md", 5, "new two"),
                ("d.md", 11, "changed"),
                ("d.md", 12, "++ plus")
            ]
        );
    }

    #[test]
    fn parses_log_records() {
        let out = "\x1eabc\x1ffeat: one\n\na.rs\nb.rs\n\x1edef\x1fdocs: two\n\nREADME.md\n";
        let commits = parse_log(out);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "abc");
        assert_eq!(commits[0].subject, "feat: one");
        assert_eq!(commits[0].files, ["a.rs", "b.rs"]);
        assert_eq!(commits[1].files, ["README.md"]);
    }

    #[test]
    fn a_subject_with_no_files_still_parses() {
        let commits = parse_log("\x1eabc\x1fchore: empty\n");
        assert_eq!(commits.len(), 1);
        assert!(commits[0].files.is_empty());
    }
}
