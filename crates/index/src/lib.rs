//! The local index of a repo's files and history that matcha's commands read
//! (docs/design.md, "The index"). For now it finds the repo; the index itself
//! comes with `census` and `recipe`.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("{0} isn't inside a git repository")]
    NotARepo(PathBuf),
    #[error("couldn't read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
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
}
