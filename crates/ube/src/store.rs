//! Results kept in `.ube/`, keyed by the exact tree they ran on, so `done`
//! can tell a green run on this code from one on code since edited. The
//! folder ignores itself (docs/ube/config.md).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::parse::Failure;
use crate::plan::Check;

/// One check's outcome on one tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub command: String,
    pub dir: String,
    pub passed: bool,
    /// `None` when it was stopped (timeout) or killed by a signal.
    pub exit: Option<i32>,
    pub secs: f64,
    pub failures: Vec<Failure>,
    /// The output's last lines, when it failed and no failure could be parsed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tail: Vec<String>,
    pub at: String,
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(root: &Path) -> Store {
        Store {
            dir: root.join(".ube"),
        }
    }

    fn file(&self, tree: &str) -> PathBuf {
        self.dir.join("results").join(format!("{tree}.json"))
    }

    /// Results recorded for `tree`, by check id.
    pub fn load(&self, tree: &str) -> BTreeMap<String, CheckResult> {
        std::fs::read_to_string(self.file(tree))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Creates `.ube/results/`, and the `.gitignore` that keeps it local.
    fn create(&self) -> Result<(), String> {
        std::fs::create_dir_all(self.dir.join("results"))
            .map_err(|e| format!("couldn't create {}: {e}", self.dir.display()))?;
        let ignore = self.dir.join(".gitignore");
        if !ignore.exists() {
            std::fs::write(&ignore, "# ube's local results; never committed.\n*\n")
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// The checks that failed in the last run, whatever tree it ran on:
    /// what `ube run --rerun-failed` runs after a fix.
    pub fn last_failed(&self) -> Vec<Check> {
        std::fs::read_to_string(self.dir.join("last-failed.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save_last_failed(&self, checks: &[Check]) -> Result<(), String> {
        self.create()?;
        let text = serde_json::to_string_pretty(checks).map_err(|e| e.to_string())?;
        std::fs::write(self.dir.join("last-failed.json"), text)
            .map_err(|e| format!("couldn't write results: {e}"))
    }

    /// Adds `results` for `tree`, replacing earlier ones for the same checks.
    pub fn save(&self, tree: &str, results: &[(String, CheckResult)]) -> Result<(), String> {
        self.create()?;
        let mut all = self.load(tree);
        for (id, r) in results {
            all.insert(id.clone(), r.clone());
        }
        let text = serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?;
        std::fs::write(self.file(tree), text).map_err(|e| format!("couldn't write results: {e}"))
    }
}
