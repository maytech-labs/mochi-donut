//! The local index of a repo's files and history that matcha's commands read
//! (docs/design.md, "The index"). It asks `git` directly rather than linking a
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
