//! `matcha ledger`: what did I verify, and what did I assume? A per-branch
//! record of claims, each verified (with evidence) or assumed, and a check
//! that flags assumed claims as they're written into docs and comments
//! (docs/design.md, "ledger"; docs/decisions/0017-ledger.md).

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use mochi_index::{AddedLine, RepoRoot};
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Verified,
    Assumed,
}

/// What backs a verified claim: a command and its output, a file and line,
/// or a note. At least one is set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl Evidence {
    /// One line for people: `kubectl get svc: 10.0.0.8` or `src/a.rs:12`.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(c) = &self.command {
            let out = self
                .output
                .as_deref()
                .map(|o| one_line(o, 80))
                .filter(|o| !o.is_empty());
            parts.push(match out {
                Some(o) => format!("`{c}`: {o}"),
                None => format!("`{c}`"),
            });
        }
        if let Some(f) = &self.file {
            parts.push(match self.line {
                Some(l) => format!("{f}:{l}"),
                None => f.clone(),
            });
        }
        if let Some(t) = &self.text {
            parts.push(t.clone());
        }
        parts.join("; ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: u32,
    pub status: Status,
    pub claim: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Evidence>,
    /// When it was recorded or last changed, UTC, RFC 3339.
    pub at: String,
}

/// A command the hook saw an agent run, with an excerpt of its output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRecord {
    pub command: String,
    pub output: String,
    pub at: String,
}

/// The ledger files for the checked-out branch, under `.matcha/ledger/`.
pub struct Store {
    dir: PathBuf,
    pub branch: String,
}

impl Store {
    pub fn open(root: &RepoRoot) -> Result<Store, String> {
        let branch = root
            .current_branch()
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| "detached-HEAD".to_owned());
        Ok(Store {
            dir: root.path().join(".matcha").join("ledger"),
            branch,
        })
    }

    /// `feat/x` is stored as `feat%2Fx`, so each branch is one file.
    fn stem(&self) -> String {
        self.branch.replace('%', "%25").replace('/', "%2F")
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(format!("{}.jsonl", self.stem()))
    }

    fn commands_path(&self) -> PathBuf {
        self.dir.join(format!("{}.commands.jsonl", self.stem()))
    }

    /// Creates `.matcha/ledger/`, and a `.matcha/.gitignore` of `*` so the
    /// ledger never ends up in a commit without anyone touching the repo's
    /// own `.gitignore`.
    fn ensure_dir(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| format!("couldn't create {}: {e}", self.dir.display()))?;
        if let Some(matcha) = self.dir.parent() {
            let ignore = matcha.join(".gitignore");
            if !ignore.exists() {
                std::fs::write(&ignore, "# matcha's local data; never committed.\n*\n")
                    .map_err(|e| format!("couldn't write {}: {e}", ignore.display()))?;
            }
        }
        Ok(())
    }

    /// Every entry, oldest first. A line that doesn't parse is an error:
    /// a ledger that silently drops claims would be worse than none.
    pub fn entries(&self) -> Result<Vec<Entry>, String> {
        read_jsonl(&self.path())
    }

    pub fn add(
        &self,
        status: Status,
        claim: &str,
        evidence: Option<Evidence>,
    ) -> Result<Entry, String> {
        let entries = self.entries()?;
        let entry = Entry {
            id: entries.iter().map(|e| e.id).max().unwrap_or(0) + 1,
            status,
            claim: claim.trim().to_owned(),
            evidence,
            at: now(),
        };
        self.ensure_dir()?;
        append_jsonl(&self.path(), &entry)?;
        Ok(entry)
    }

    /// Marks entry `id` verified, with `evidence`.
    pub fn verify(&self, id: u32, evidence: Evidence) -> Result<Entry, String> {
        let mut entries = self.entries()?;
        let entry = entries.iter_mut().find(|e| e.id == id).ok_or_else(|| {
            format!(
                "no entry {id} in the ledger for {}. `matcha ledger list` shows them.",
                self.branch
            )
        })?;
        entry.status = Status::Verified;
        entry.evidence = Some(evidence);
        entry.at = now();
        let updated = entry.clone();
        write_jsonl(&self.path(), &entries)?;
        Ok(updated)
    }

    /// Moves the branch's ledger to `archive/<branch>-<date>.jsonl`.
    pub fn archive(&self) -> Result<Option<PathBuf>, String> {
        let from = self.path();
        if !from.exists() {
            return Ok(None);
        }
        let archive = self.dir.join("archive");
        std::fs::create_dir_all(&archive)
            .map_err(|e| format!("couldn't create {}: {e}", archive.display()))?;
        let date = now().get(..10).unwrap_or("undated").to_owned();
        let mut to = archive.join(format!("{}-{date}.jsonl", self.stem()));
        let mut n = 2;
        while to.exists() {
            to = archive.join(format!("{}-{date}-{n}.jsonl", self.stem()));
            n += 1;
        }
        std::fs::rename(&from, &to)
            .map_err(|e| format!("couldn't move {}: {e}", from.display()))?;
        let _ = std::fs::remove_file(self.commands_path());
        Ok(Some(to))
    }

    /// Records a command the agent ran. Keeps the file under about 1 MB by
    /// dropping the oldest records.
    pub fn record_command(&self, command: &str, output: &str) -> Result<(), String> {
        self.ensure_dir()?;
        let path = self.commands_path();
        append_jsonl(
            &path,
            &CommandRecord {
                command: command.to_owned(),
                output: excerpt(&redact(output)),
                at: now(),
            },
        )?;
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > 1024 * 1024) {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let lines: Vec<&str> = text.lines().collect();
            let keep = lines[lines.len().saturating_sub(300)..].join("\n");
            std::fs::write(&path, keep + "\n").map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// The most recent recorded command containing `needle`.
    pub fn find_command(&self, needle: &str) -> Result<Option<CommandRecord>, String> {
        let records: Vec<CommandRecord> = read_jsonl(&self.commands_path())?;
        Ok(records
            .into_iter()
            .rev()
            .find(|r| r.command.contains(needle)))
    }
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("couldn't read {}: {e}", path.display())),
    };
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| {
            serde_json::from_str(l).map_err(|e| {
                format!(
                    "{}:{} isn't a valid ledger line ({e}). Fix or remove that line.",
                    path.display(),
                    i + 1
                )
            })
        })
        .collect()
}

fn append_jsonl<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let line = serde_json::to_string(value).map_err(|e| e.to_string())?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("couldn't open {}: {e}", path.display()))?;
    writeln!(f, "{line}").map_err(|e| format!("couldn't write {}: {e}", path.display()))
}

fn write_jsonl<T: Serialize>(path: &Path, values: &[T]) -> Result<(), String> {
    let mut text = String::new();
    for v in values {
        text.push_str(&serde_json::to_string(v).map_err(|e| e.to_string())?);
        text.push('\n');
    }
    // Write beside it, then rename, so a crash can't leave half a ledger.
    let tmp = path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("couldn't write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("couldn't replace {}: {e}", path.display()))
}

/// Evidence from a file and optional line: `src/a.rs:12`. The line's text is
/// kept as the excerpt, so the evidence survives later edits.
pub fn file_evidence(root: &Path, spec: &str) -> Result<Evidence, String> {
    let (file, line) = match spec.rsplit_once(':') {
        Some((f, l)) if l.chars().all(|c| c.is_ascii_digit()) && !l.is_empty() => {
            (f, l.parse::<usize>().ok())
        }
        _ => (spec, None),
    };
    let text = std::fs::read_to_string(root.join(file))
        .map_err(|e| format!("--file {file}: {e}. Give a path relative to the repo root."))?;
    let excerpt = match line {
        Some(n) => Some(
            text.lines()
                .nth(n.saturating_sub(1))
                .ok_or_else(|| {
                    format!(
                        "--file {spec}: {file} has only {} lines",
                        text.lines().count()
                    )
                })?
                .trim()
                .to_owned(),
        ),
        None => None,
    };
    Ok(Evidence {
        file: Some(file.to_owned()),
        line,
        output: excerpt,
        ..Evidence::default()
    })
}

/// An assumed claim that an added line in a doc or comment states.
#[derive(Debug, Serialize)]
pub struct Flag {
    pub path: String,
    pub line: usize,
    pub text: String,
    pub id: u32,
    pub claim: String,
    /// The claim's words the line shares, out of the claim's words.
    pub shared: usize,
    pub of: usize,
}

/// Assumed claims that show up in added lines of docs or comments, matched
/// by the share of the claim's words the line contains.
pub fn check(entries: &[Entry], added: &[AddedLine]) -> Vec<Flag> {
    let assumed: Vec<(&Entry, BTreeSet<String>)> = entries
        .iter()
        .filter(|e| e.status == Status::Assumed)
        .map(|e| (e, words(&e.claim)))
        .filter(|(_, w)| !w.is_empty())
        .collect();
    let mut flags = Vec::new();
    for line in added.iter().filter(|l| is_prose(&l.path, &l.text)) {
        let line_words = words(&line.text);
        let best = assumed
            .iter()
            .map(|(e, w)| (e, w.intersection(&line_words).count(), w.len()))
            .filter(|(_, shared, of)| *shared >= (*of).min(3) && *shared as f64 / *of as f64 >= 0.6)
            .max_by(|a, b| {
                (a.1 as f64 / a.2 as f64)
                    .total_cmp(&(b.1 as f64 / b.2 as f64))
                    .then(b.0.id.cmp(&a.0.id))
            });
        if let Some((e, shared, of)) = best {
            flags.push(Flag {
                path: line.path.clone(),
                line: line.line,
                text: line.text.trim().to_owned(),
                id: e.id,
                claim: e.claim.clone(),
                shared,
                of,
            });
        }
    }
    flags
}

/// Whether an added line is prose: in a doc, or a comment in code.
fn is_prose(path: &str, text: &str) -> bool {
    const DOCS: &[&str] = &[".md", ".mdx", ".rst", ".txt", ".adoc"];
    const COMMENTS: &[&str] = &[
        "//", "#", "/*", "*", "--", "<!--", ";", "%", "\"\"\"", "'''",
    ];
    let t = text.trim_start();
    !t.is_empty()
        && (DOCS.iter().any(|d| path.ends_with(d)) || COMMENTS.iter().any(|c| t.starts_with(c)))
}

/// A claim's or line's meaningful words: lowercased, split at anything but
/// letters and digits, without common short words.
fn words(text: &str) -> BTreeSet<String> {
    const STOP: &[&str] = &[
        "a", "an", "the", "in", "on", "of", "to", "is", "are", "was", "were", "be", "and", "or",
        "for", "with", "by", "it", "its", "that", "this", "as", "at", "from", "we", "our", "all",
        "any", "has", "have", "use", "uses", "used",
    ];
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !STOP.contains(w))
        .map(|w| {
            w.strip_suffix('s')
                .filter(|s| s.len() > 2)
                .unwrap_or(w)
                .to_owned()
        })
        .collect()
}

/// The ledger as Markdown for a PR description.
pub fn summary(entries: &[Entry]) -> String {
    let mut out = String::new();
    for (title, status) in [("Verified", Status::Verified), ("Assumed", Status::Assumed)] {
        let group: Vec<&Entry> = entries.iter().filter(|e| e.status == status).collect();
        if group.is_empty() {
            continue;
        }
        out.push_str(&format!("{title}:\n"));
        for e in group {
            match e
                .evidence
                .as_ref()
                .map(Evidence::describe)
                .filter(|d| !d.is_empty())
            {
                Some(d) => out.push_str(&format!("- {} ({d})\n", e.claim)),
                None => out.push_str(&format!("- {}\n", e.claim)),
            }
        }
        out.push('\n');
    }
    out.trim_end().to_owned()
}

/// Hides values that look like secrets: `token=abc` becomes `token=[redacted]`.
pub fn redact(text: &str) -> String {
    let re = Regex::new(
        r"(?i)((?:secret|token|password|passwd|api[_-]?key|private[_-]?key|credential)[\w.-]*\s*[=:]\s*)\S+",
    );
    match re {
        Ok(re) => re.replace_all(text, "${1}[redacted]").into_owned(),
        // The pattern is a literal; if it ever fails to build, keep nothing.
        Err(_) => String::new(),
    }
}

/// The first 1500 and last 500 characters of a long output.
fn excerpt(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= 2000 {
        return text.to_owned();
    }
    let head: String = chars[..1500].iter().collect();
    let tail: String = chars[chars.len() - 500..].iter().collect();
    format!(
        "{head}\n[... {} characters left out ...]\n{tail}",
        chars.len() - 2000
    )
}

/// `text` on one line, cut to `max` characters.
fn one_line(text: &str, max: usize) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() <= max {
        joined
    } else {
        format!("{}...", joined.chars().take(max).collect::<String>())
    }
}

/// The current time, UTC, as RFC 3339 (`2026-10-12T14:58:03Z`).
pub fn now() -> String {
    mochi_common::time::now_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u32, status: Status, claim: &str) -> Entry {
        Entry {
            id,
            status,
            claim: claim.into(),
            evidence: None,
            at: "2026-10-12T00:00:00Z".into(),
        }
    }

    fn added(path: &str, line: usize, text: &str) -> AddedLine {
        AddedLine {
            path: path.into(),
            line,
            text: text.into(),
        }
    }

    #[test]
    fn flags_assumed_claims_written_into_docs() {
        let entries = [
            entry(1, Status::Assumed, "FEINs in tests use a 00- prefix"),
            entry(2, Status::Verified, "Traefik listens on all three nodes"),
            entry(3, Status::Assumed, "The cluster has three nodes"),
        ];
        let lines = [
            added("docs/test-data.md", 11, "FEINs use the 00- prefix."),
            added("docs/net.md", 3, "Traefik listens on all three nodes."),
            added(
                "src/a.rs",
                9,
                "let fein = \"00-1234567\"; // FEINs use the 00- prefix",
            ),
            added("src/a.rs", 10, "let prefix = \"00-\"; let feins = 3;"),
            added("docs/other.md", 1, "Tests are fast."),
        ];
        let flags = check(&entries, &lines);
        let got: Vec<(&str, usize, u32)> = flags
            .iter()
            .map(|f| (f.path.as_str(), f.line, f.id))
            .collect();
        assert_eq!(got, [("docs/test-data.md", 11, 1)]);
        assert_eq!((flags[0].shared, flags[0].of), (3, 4));
    }

    #[test]
    fn comments_count_as_prose() {
        assert!(is_prose("src/a.rs", "  // FEINs use 00-"));
        assert!(is_prose("a.py", "# note"));
        assert!(is_prose("README.md", "anything"));
        assert!(!is_prose("src/a.rs", "let x = 1; // trailing"));
    }

    #[test]
    fn summarizes_for_a_pr() {
        let mut v = entry(1, Status::Verified, "Traefik listens on all three nodes");
        v.evidence = Some(Evidence {
            command: Some("kubectl get svc -A".into()),
            output: Some("traefik  10.0.0.127,10.0.0.24,10.0.0.8".into()),
            ..Evidence::default()
        });
        let s = summary(&[v, entry(2, Status::Assumed, "FEINs use a 00- prefix")]);
        assert_eq!(
            s,
            "Verified:\n- Traefik listens on all three nodes (`kubectl get svc -A`: traefik 10.0.0.127,10.0.0.24,10.0.0.8)\n\nAssumed:\n- FEINs use a 00- prefix"
        );
    }

    #[test]
    fn redacts_secret_looking_values() {
        assert_eq!(
            redact("user=bob\nVAULT_TOKEN=hvs.abc password: hunter2"),
            "user=bob\nVAULT_TOKEN=[redacted] password: [redacted]"
        );
    }

    #[test]
    fn long_output_keeps_head_and_tail() {
        let long = "a".repeat(1500) + &"b".repeat(1000) + &"c".repeat(500);
        let e = excerpt(&long);
        assert!(e.starts_with(&"a".repeat(1500)));
        assert!(e.ends_with(&"c".repeat(500)));
        assert!(e.contains("[... 1000 characters left out ...]"));
    }
}
