//! Which files change together, counted from history.

use std::collections::HashMap;

use crate::Commit;

/// How often each file changed, and how often each pair changed in the same
/// commit. Files are interned to small integer ids.
#[derive(Debug, Default)]
pub struct CoChange {
    ids: HashMap<String, u32>,
    names: Vec<String>,
    /// Commits that changed each file, by id.
    counts: Vec<u32>,
    /// For each file id, the files it changed with and how many times.
    neighbors: Vec<HashMap<u32, u32>>,
    commits: usize,
}

/// A file that usually changes with another one.
#[derive(Debug, Clone, PartialEq)]
pub struct Companion {
    pub file: String,
    /// Commits that changed both files.
    pub together: u32,
    /// Commits that changed the first file.
    pub of: u32,
    /// How much likelier the companion is to change given the first file
    /// changed, than in any commit. 1 means no connection.
    pub lift: f64,
}

impl Companion {
    pub fn confidence(&self) -> f64 {
        f64::from(self.together) / f64::from(self.of)
    }
}

impl CoChange {
    /// Counts co-changes over `commits`, skipping any commit that changed more
    /// than `max_files` files (mass reformatting, vendoring, renames).
    pub fn build(commits: &[Commit], max_files: usize) -> CoChange {
        let mut cc = CoChange::default();
        for commit in commits {
            if commit.files.is_empty() || commit.files.len() > max_files {
                continue;
            }
            cc.commits += 1;
            let mut ids: Vec<u32> = commit.files.iter().map(|f| cc.intern(f)).collect();
            ids.sort_unstable();
            ids.dedup();
            for &a in &ids {
                cc.counts[a as usize] += 1;
                for &b in &ids {
                    if a != b {
                        *cc.neighbors[a as usize].entry(b).or_insert(0) += 1;
                    }
                }
            }
        }
        cc
    }

    fn intern(&mut self, file: &str) -> u32 {
        if let Some(&id) = self.ids.get(file) {
            return id;
        }
        let id = u32::try_from(self.names.len()).unwrap_or(u32::MAX);
        self.ids.insert(file.to_owned(), id);
        self.names.push(file.to_owned());
        self.counts.push(0);
        self.neighbors.push(HashMap::new());
        id
    }

    /// Commits counted (after skipping oversized ones).
    pub fn commits(&self) -> usize {
        self.commits
    }

    /// Commits that changed `file`.
    pub fn count(&self, file: &str) -> u32 {
        self.ids.get(file).map_or(0, |&id| self.counts[id as usize])
    }

    /// Files that changed with `file` in at least `min_confidence` of the at
    /// least `min_support` commits that changed it, and are at least
    /// `min_lift` times likelier to change with it than without it. Most
    /// reliable first.
    pub fn companions(
        &self,
        file: &str,
        min_support: u32,
        min_confidence: f64,
        min_lift: f64,
    ) -> Vec<Companion> {
        let Some(&a) = self.ids.get(file) else {
            return Vec::new();
        };
        let of = self.counts[a as usize];
        if of < min_support {
            return Vec::new();
        }
        let mut out: Vec<Companion> = self.neighbors[a as usize]
            .iter()
            .map(|(&other, &together)| {
                let base = f64::from(self.counts[other as usize]) / self.commits as f64;
                Companion {
                    file: self.names[other as usize].clone(),
                    together,
                    of,
                    lift: (f64::from(together) / f64::from(of)) / base,
                }
            })
            .filter(|c| c.confidence() >= min_confidence && c.lift >= min_lift)
            .collect();
        out.sort_by(|p, q| {
            q.together
                .cmp(&p.together)
                .then_with(|| p.file.cmp(&q.file))
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(files: &[&str]) -> Commit {
        Commit {
            hash: String::new(),
            subject: String::new(),
            files: files.iter().map(|f| (*f).to_owned()).collect(),
        }
    }

    #[test]
    fn counts_files_and_pairs() {
        let cc = CoChange::build(
            &[
                commit(&["api.rs", "spec.json"]),
                commit(&["api.rs", "spec.json", "README.md"]),
                commit(&["api.rs"]),
            ],
            100,
        );
        assert_eq!(cc.commits(), 3);
        assert_eq!(cc.count("api.rs"), 3);
        let c = cc.companions("api.rs", 1, 0.5, 0.0);
        assert_eq!(c[0].file, "spec.json");
        assert_eq!((c[0].together, c[0].of), (2, 3));
        // README changed with api.rs once in three: below 50%.
        assert!(c.iter().all(|c| c.file != "README.md"));
    }

    #[test]
    fn needs_enough_history() {
        let cc = CoChange::build(&[commit(&["a", "b"]), commit(&["a", "b"])], 100);
        assert!(cc.companions("a", 3, 0.9, 0.0).is_empty());
        assert_eq!(cc.companions("a", 2, 0.9, 0.0).len(), 1);
    }

    #[test]
    fn skips_oversized_commits() {
        let cc = CoChange::build(&[commit(&["a", "b", "c"]), commit(&["a", "b"])], 2);
        assert_eq!(cc.commits(), 1);
        assert_eq!(cc.count("c"), 0);
    }

    #[test]
    fn unknown_files_have_no_companions() {
        let cc = CoChange::build(&[commit(&["a", "b"])], 100);
        assert!(cc.companions("zzz", 1, 0.0, 0.0).is_empty());
    }

    #[test]
    fn files_that_change_in_every_commit_predict_nothing() {
        // `notes.md` changes every time, so it rides along with `a` by chance.
        let cc = CoChange::build(
            &[
                commit(&["a", "spec", "notes.md"]),
                commit(&["a", "spec", "notes.md"]),
                commit(&["b", "notes.md"]),
                commit(&["c", "notes.md"]),
            ],
            100,
        );
        let c = cc.companions("a", 2, 0.9, 1.5);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].file, "spec");
        assert!((c[0].lift - 2.0).abs() < 1e-9);
    }
}
