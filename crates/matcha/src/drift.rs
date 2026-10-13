//! `matcha drift`: are the docs still true? Finds mechanical claims in
//! Markdown (code spans naming paths, symbols, make targets, and npm scripts;
//! links and their anchors) and checks each against the repo
//! (docs/design.md, "drift"; docs/decisions/0015-drift-mechanical-claims.md).

use std::collections::{BTreeSet, HashMap, HashSet};

use mochi_index::RepoRoot;
use serde::Serialize;

use crate::files::SourceFile;
use crate::markdown::{self, Link, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A code span naming a file or directory.
    Path,
    /// A code span naming a type, function, or other identifier.
    Symbol,
    /// `make <target>`.
    MakeTarget,
    /// `npm run <script>` (or pnpm, yarn, bun).
    NpmScript,
    /// A link to a file in the repo.
    Link,
    /// A link to a heading in a Markdown file.
    Anchor,
}

/// A claim that's no longer true.
#[derive(Debug, Serialize)]
pub struct Failure {
    pub doc: String,
    pub line: usize,
    pub kind: Kind,
    /// The code span or link target, as written.
    pub claim: String,
    pub reason: String,
    pub fix: String,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub docs_checked: usize,
    pub source_files_searched: usize,
    pub claims_checked: usize,
    pub holds: usize,
    pub failing: Vec<Failure>,
    /// Code spans and links that make no checkable claim, or that point
    /// outside the repo: prose in backticks, URLs, ignored files.
    pub skipped: usize,
}

/// Whether a doc records the past (a changelog, ADRs) or plans the future (a
/// roadmap, requirements, proposals). Its code spans may name things that are
/// gone, or not built yet, on purpose. Its links still count.
pub fn is_past_or_future(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_uppercase();
    [
        "CHANGE", "HISTORY", "NEWS", "RELEASE", "WHATSNEW", "ROADMAP", "TODO",
    ]
    .iter()
    .any(|p| name.starts_with(p))
        || path.split('/').any(|seg| {
            matches!(
                seg,
                "decisions" | "adr" | "adrs" | "rfcs" | "proposals" | "requirements" | "roadmap"
            )
        })
}

pub fn is_doc(path: &str) -> bool {
    path.ends_with(".md") || path.ends_with(".mdx")
}

/// Directories below the root with their own license file. Docs under them
/// are usually vendored copies of someone else's project.
pub fn licensed_dirs(tracked: &[String]) -> BTreeSet<String> {
    tracked
        .iter()
        .filter_map(|f| {
            let (dir, name) = f.rsplit_once('/')?;
            let upper = name.to_ascii_uppercase();
            ["LICENSE", "LICENCE", "COPYING"]
                .iter()
                .any(|p| upper.starts_with(p))
                .then(|| dir.to_owned())
        })
        .collect()
}

/// Whether a doc is the repo's own documentation, not a test fixture or a
/// vendored copy of someone else's (under one of `licensed` dirs).
pub fn is_own_doc(path: &str, licensed: &BTreeSet<String>) -> bool {
    let segs: Vec<&str> = path.split('/').collect();
    let vendored = (1..segs.len()).any(|n| licensed.contains(&segs[..n].join("/")));
    is_doc(path)
        && !vendored
        && !segs.iter().any(|s| {
            matches!(
                *s,
                "testdata"
                    | "testData"
                    | "test-data"
                    | "test_data"
                    | "fixtures"
                    | "__fixtures__"
                    | "__snapshots__"
                    | "vendor"
                    | "third_party"
                    | "third-party"
                    | "node_modules"
            )
        })
        && !segs
            .windows(2)
            .any(|w| matches!(w[0], "test" | "tests" | "testSrc") && w[1] == "data")
}

/// What a code span claims, before it's checked.
enum Claim<'s> {
    Make(Vec<&'s str>),
    Script(&'s str),
    Path(&'s str),
    Symbol(&'s str),
}

/// Everything the checks look things up in, built once per run.
pub struct Repo<'a> {
    root: &'a RepoRoot,
    files: BTreeSet<String>,
    dirs: BTreeSet<String>,
    tokens: HashSet<String>,
    source_files: usize,
    make_targets: Option<HashSet<String>>,
    npm_scripts: Option<HashSet<String>>,
    anchors: HashMap<String, BTreeSet<String>>,
}

impl<'a> Repo<'a> {
    /// `all` is every tracked text file; docs are told apart by extension.
    /// `docs` are the ones to check, whose symbol claims are looked up.
    pub fn new(
        root: &'a RepoRoot,
        tracked: Vec<String>,
        all: &[SourceFile],
        docs: &[&SourceFile],
    ) -> Self {
        let mut dirs = BTreeSet::new();
        for f in &tracked {
            let mut p = f.as_str();
            while let Some((parent, _)) = p.rsplit_once('/') {
                if !dirs.insert(parent.to_owned()) {
                    break;
                }
                p = parent;
            }
        }
        let mut source_files = 0;
        let mut make_targets: Option<HashSet<String>> = None;
        let mut npm_scripts: Option<HashSet<String>> = None;
        let mut anchors = HashMap::new();
        for file in all {
            if is_doc(&file.path) {
                anchors.insert(file.path.clone(), markdown::parse(&file.text).anchors);
                continue;
            }
            if !is_source(&file.path) {
                continue;
            }
            source_files += 1;
            let name = file.path.rsplit('/').next().unwrap_or(&file.path);
            if matches!(name, "Makefile" | "makefile" | "GNUmakefile") || name.ends_with(".mk") {
                make_targets
                    .get_or_insert_with(HashSet::new)
                    .extend(targets(&file.text));
            }
            if name == "package.json" {
                let scripts = npm_scripts.get_or_insert_with(HashSet::new);
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&file.text)
                    && let Some(obj) = v.get("scripts").and_then(|s| s.as_object())
                {
                    scripts.extend(obj.keys().cloned());
                }
            }
        }
        let mut repo = Repo {
            root,
            files: tracked.into_iter().collect(),
            dirs,
            tokens: HashSet::new(),
            source_files,
            make_targets,
            npm_scripts,
            anchors,
        };
        repo.tokens = repo.find_symbols(all, docs);
        repo
    }

    /// The symbol names `docs` claim that appear in a source file. Scanning
    /// for just these, not every identifier, keeps big repos fast.
    fn find_symbols(&self, all: &[SourceFile], docs: &[&SourceFile]) -> HashSet<String> {
        let mut wanted: HashSet<&str> = HashSet::new();
        let parsed: Vec<(&str, markdown::Doc)> = docs
            .iter()
            .filter(|d| !is_past_or_future(&d.path))
            .map(|d| (d.path.as_str(), markdown::parse(&d.text)))
            .collect();
        for (doc, p) in &parsed {
            if p.links_only {
                continue;
            }
            for span in &p.spans {
                if let Some(Claim::Symbol(name)) = self.classify(doc, span) {
                    wanted.insert(name);
                }
            }
        }
        let mut found = HashSet::new();
        for file in all.iter().filter(|f| is_source(&f.path)) {
            if found.len() == wanted.len() {
                break;
            }
            for word in identifiers(&file.text) {
                if wanted.contains(word) && !found.contains(word) {
                    found.insert(word.to_owned());
                }
            }
        }
        found
    }

    /// Checks every claim in `docs`.
    pub fn check(&self, docs: &[&SourceFile]) -> Report {
        let mut all: Vec<(&str, usize, Kind, String, Outcome)> = Vec::new();
        let mut skipped = 0;
        for doc in docs {
            let parsed = markdown::parse(&doc.text);
            let links_only = is_past_or_future(&doc.path) || parsed.links_only;
            let mut outcomes: Vec<(usize, Kind, String, Outcome)> = Vec::new();
            for span in &parsed.spans {
                match (!links_only)
                    .then(|| self.check_span(&doc.path, span))
                    .flatten()
                {
                    Some((kind, outcome)) => {
                        outcomes.push((span.line, kind, span.code.clone(), outcome));
                    }
                    None => skipped += 1,
                }
            }
            for link in &parsed.links {
                match self.check_link(&doc.path, link) {
                    Some((kind, outcome)) => {
                        outcomes.push((link.line, kind, link.target.clone(), outcome));
                    }
                    None => skipped += 1,
                }
            }
            // Spans before links on the same line; otherwise in line order.
            // A claim repeated on one line is reported once.
            outcomes.sort_by_key(|o| o.0);
            let mut seen = HashSet::new();
            outcomes.retain(|o| seen.insert((o.0, o.2.clone())));
            all.extend(
                outcomes
                    .into_iter()
                    .map(|(line, kind, claim, o)| (doc.path.as_str(), line, kind, claim, o)),
            );
        }

        // Missing paths that git ignores (`.env`, `target/`) may exist on
        // other machines. Ask git about all of them at once.
        let unresolved: Vec<&str> = all
            .iter()
            .filter_map(|o| match &o.4 {
                Outcome::Missing { paths, .. } => Some(paths),
                _ => None,
            })
            .flatten()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let ignored = self.ignored(&unresolved);

        let mut failing = Vec::new();
        let mut holds = 0;
        for (doc, line, kind, claim, outcome) in all {
            let (reason, fix) = match outcome {
                Outcome::Holds => {
                    holds += 1;
                    continue;
                }
                Outcome::Missing { paths, .. } if paths.iter().any(|p| ignored.contains(p)) => {
                    skipped += 1;
                    continue;
                }
                Outcome::Fails { reason, fix } | Outcome::Missing { reason, fix, .. } => {
                    (reason, fix)
                }
            };
            failing.push(Failure {
                doc: doc.to_owned(),
                line,
                kind,
                claim,
                reason,
                fix,
            });
        }
        Report {
            docs_checked: docs.len(),
            source_files_searched: self.source_files,
            claims_checked: holds + failing.len(),
            holds,
            failing,
            skipped,
        }
    }

    /// What a code span claims, or `None` if it makes no checkable claim.
    fn classify<'s>(&self, doc: &str, span: &'s Span) -> Option<Claim<'s>> {
        if negated(&span.before, &span.after) || example(&span.before) || todo(&span.before) {
            return None;
        }
        let code = span.code.trim();
        let words: Vec<&str> = code.split_whitespace().collect();
        match words.as_slice() {
            ["make", rest @ ..] => {
                let targets: Vec<&str> = rest
                    .iter()
                    .copied()
                    .filter(|a| !a.contains('=') && !a.starts_with('-'))
                    .collect();
                if targets.is_empty() || targets.iter().any(|t| placeholder(t)) {
                    return None;
                }
                return Some(Claim::Make(targets));
            }
            ["npm" | "pnpm" | "yarn" | "bun", "run", script, ..] => {
                return Some(Claim::Script(script));
            }
            [_] => {}
            _ => return None,
        }
        if placeholder(code) {
            return None;
        }
        if looks_like_path(code, |first| self.exists_near(doc, first)) {
            return Some(Claim::Path(code));
        }
        symbol_name(code).map(Claim::Symbol)
    }

    fn check_span(&self, doc: &str, span: &Span) -> Option<(Kind, Outcome)> {
        let name = match self.classify(doc, span)? {
            Claim::Make(targets) => return Some((Kind::MakeTarget, self.check_make(&targets))),
            Claim::Script(script) => return Some((Kind::NpmScript, self.check_script(script))),
            Claim::Path(path) => return self.check_path(doc, path).map(|o| (Kind::Path, o)),
            Claim::Symbol(name) => name,
        };
        let outcome = if self.tokens.contains(name) {
            Outcome::Holds
        } else {
            Outcome::Fails {
                reason: format!(
                    "`{name}` appears in no source file ({} searched)",
                    self.source_files
                ),
                fix: "Update the doc to name what the code uses now. If it names something outside this repo, put <!-- matcha: skip --> on the line before.".into(),
            }
        };
        Some((Kind::Symbol, outcome))
    }

    fn check_make(&self, wanted: &[&str]) -> Outcome {
        let Some(targets) = &self.make_targets else {
            return fails(
                "the repo has no Makefile",
                "Name the command that does this now, or add the Makefile target.",
            );
        };
        let missing: Vec<&str> = wanted
            .iter()
            .copied()
            .filter(|t| !targets.contains(*t))
            .collect();
        if missing.is_empty() {
            Outcome::Holds
        } else {
            let mut known: Vec<&str> = targets.iter().map(String::as_str).collect();
            known.sort_unstable();
            Outcome::Fails {
                reason: format!("no Makefile defines `{}`", missing.join("`, `")),
                fix: closest(missing[0], &known).map_or_else(
                    || format!("The targets are: {}.", known.join(", ")),
                    |c| format!("Did you mean `make {c}`?"),
                ),
            }
        }
    }

    fn check_script(&self, script: &str) -> Outcome {
        let Some(scripts) = &self.npm_scripts else {
            return fails(
                "the repo has no package.json",
                "Name the command that does this now.",
            );
        };
        if scripts.contains(script) {
            return Outcome::Holds;
        }
        let mut known: Vec<&str> = scripts.iter().map(String::as_str).collect();
        known.sort_unstable();
        Outcome::Fails {
            reason: format!("no package.json has a `{script}` script"),
            fix: closest(script, &known).map_or_else(
                || format!("The scripts are: {}.", known.join(", ")),
                |c| format!("Did you mean `npm run {c}`?"),
            ),
        }
    }

    /// A path in a code span: relative to the doc, to the repo root, or the
    /// tail of any tracked path (`lib/money.ts` for `frontend/src/lib/money.ts`).
    fn check_path(&self, doc: &str, code: &str) -> Option<Outcome> {
        let p = code.trim_start_matches("./").trim_end_matches('/');
        let mut candidates: Vec<String> = Vec::new();
        if let Some(near) = join(parent(doc), p) {
            candidates.push(near);
        }
        if !code.starts_with("../")
            && let Some(at_root) = join("", p)
        {
            candidates.push(at_root);
        }
        if candidates.is_empty() {
            return None; // Climbs out of the repo.
        }
        if candidates.iter().any(|c| self.exists(c)) {
            return Some(Outcome::Holds);
        }
        let tail = format!("/{}", candidates.last().map_or(p, String::as_str));
        if self
            .files
            .iter()
            .chain(&self.dirs)
            .any(|f| f.ends_with(&tail))
        {
            return Some(Outcome::Holds);
        }
        // Ask git about the directory form too: `target/` ignores only dirs.
        let paths = candidates
            .iter()
            .flat_map(|c| [c.clone(), format!("{c}/")])
            .collect();
        Some(Outcome::Missing {
            reason: format!("no file or directory `{p}` in the repo"),
            fix: self.same_name_hint(p),
            paths,
        })
    }

    fn check_link(&self, doc: &str, link: &Link) -> Option<(Kind, Outcome)> {
        let target = link.target.trim();
        if target.contains("://")
            || target.starts_with("//")
            || target.starts_with("mailto:")
            || target.starts_with("tel:")
            || target.contains('{')
            || placeholder_words(target)
        {
            return None;
        }
        let (path, fragment) = match target.split_once('#') {
            Some((p, f)) => (p, Some(f)),
            None => (target, None),
        };
        let path = path.split('?').next().unwrap_or("");
        let path = percent_decode(path);
        let resolved = if path.is_empty() {
            doc.to_owned()
        } else if let Some(abs) = path.strip_prefix('/') {
            join("", abs)?
        } else {
            join(parent(doc), &path)?
        };
        if !self.exists(&resolved) {
            return Some((
                Kind::Link,
                Outcome::Missing {
                    reason: format!("`{resolved}` doesn't exist"),
                    fix: self.same_name_hint(&resolved),
                    paths: vec![resolved],
                },
            ));
        }
        let Some(fragment) = fragment.filter(|f| !f.is_empty()) else {
            return Some((Kind::Link, Outcome::Holds));
        };
        let Some(anchors) = self.anchors.get(&resolved) else {
            // Line anchors in code, or a doc we didn't read: can't tell.
            return Some((Kind::Link, Outcome::Holds));
        };
        let fragment = percent_decode(fragment).to_lowercase();
        if anchors.contains(&fragment) {
            return Some((Kind::Anchor, Outcome::Holds));
        }
        let known: Vec<&str> = anchors.iter().map(String::as_str).collect();
        let fix = match closest(&fragment, &known) {
            Some(c) => format!("The closest heading is `#{c}`."),
            None if known.is_empty() => format!("`{resolved}` has no headings to link to."),
            None => "Link to one of its headings, or drop the anchor.".into(),
        };
        Some((
            Kind::Anchor,
            Outcome::Fails {
                reason: format!("`{resolved}` has no heading `#{fragment}`"),
                fix,
            },
        ))
    }

    /// Tracked, a parent of something tracked, or on disk.
    fn exists(&self, path: &str) -> bool {
        path.is_empty()
            || self.files.contains(path)
            || self.dirs.contains(path)
            || self.root.path().join(path).exists()
    }

    /// Whether the first segment of a path exists beside the doc or at the
    /// root, which marks `crates/foo` as a path even without an extension.
    /// Tracked only, not on disk: macOS matches names case-insensitively, so
    /// `ghidra/ghidra` would match the `Ghidra` directory.
    fn exists_near(&self, doc: &str, first: &str) -> bool {
        [join(parent(doc), first), join("", first)]
            .into_iter()
            .flatten()
            .any(|p| self.files.contains(&p) || self.dirs.contains(&p))
    }

    /// Which of `paths` git ignores. If git can't say, none: report the
    /// claim rather than hide it.
    fn ignored(&self, paths: &[&str]) -> HashSet<String> {
        paths
            .chunks(500)
            .filter_map(|chunk| self.root.ignored(chunk).ok())
            .flatten()
            .collect()
    }

    fn same_name_hint(&self, path: &str) -> String {
        let name = path.rsplit('/').next().unwrap_or(path);
        let suffix = format!("/{name}");
        let same: Vec<&str> = self
            .files
            .iter()
            .filter(|f| f.ends_with(&suffix) || f.as_str() == name)
            .map(String::as_str)
            .take(3)
            .collect();
        if same.is_empty() {
            "Fix the path, or drop the claim if the file is gone.".into()
        } else {
            format!("Did you mean `{}`?", same.join("`, `"))
        }
    }
}

/// Code and config, where symbols are looked up: everything but prose.
fn is_source(path: &str) -> bool {
    !is_doc(path) && !path.ends_with(".txt") && !path.ends_with(".rst")
}

/// ASCII identifiers in `text`: `[A-Za-z_][A-Za-z0-9_]*`.
fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    let bytes = text.as_bytes();
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < bytes.len() {
            let b = bytes[i];
            let starts = (b.is_ascii_alphabetic() || b == b'_')
                && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_'));
            if !starts {
                i += 1;
                continue;
            }
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            // ASCII bytes only, so these are char boundaries.
            return text.get(start..i);
        }
        None
    })
}

enum Outcome {
    Holds,
    Fails {
        reason: String,
        fix: String,
    },
    /// Fails unless git ignores one of `paths`.
    Missing {
        paths: Vec<String>,
        reason: String,
        fix: String,
    },
}

fn fails(reason: &str, fix: &str) -> Outcome {
    Outcome::Fails {
        reason: reason.into(),
        fix: fix.into(),
    }
}

/// Whether the clause around a code span says it's not used: "`a`, not `b`",
/// "rather than `b`", "`b` was removed". Such spans are skipped.
fn negated(before: &str, after: &str) -> bool {
    const BEFORE: &[&str] = &[
        "not",
        "never",
        "no",
        "don't",
        "dont",
        "doesn't",
        "isn't",
        "aren't",
        "avoid",
        "without",
        "instead",
        "rather",
        "nor",
        "deprecated",
        "removed",
        "replaces",
        "replaced",
        "formerly",
        "previously",
        "legacy",
        "forbid",
        "forbidden",
        "ban",
        "banned",
        "disallow",
        "disallowed",
        "unlike",
        "except",
        "planned",
        "future",
        "todo",
        "eventually",
        "someday",
    ];
    const AFTER: &[&str] = &[
        "removed",
        "deleted",
        "deprecated",
        "renamed",
        "replaced",
        "gone",
        "obsolete",
        "yet",
        "planned",
        "eventually",
    ];
    const AFTER_PHRASES: &[&str] = &[
        "no longer",
        "once it exists",
        "when it exists",
        "once they exist",
        "when they exist",
    ];
    let clause_start = clause_breaks(before).last().map_or(0, |i| i + 1);
    let clause_end = clause_breaks(after).next().unwrap_or(after.len());
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| !(c.is_alphabetic() || c == '\'' || c == '’'))
            .filter(|w| !w.is_empty())
            .map(|w| w.to_lowercase().replace('’', "'"))
            .collect()
    };
    let after_words = words(&after[..clause_end]);
    let after_text = after_words.join(" ");
    words(&before[clause_start..])
        .iter()
        .any(|w| BEFORE.contains(&w.as_str()))
        || after_words.iter().any(|w| AFTER.contains(&w.as_str()))
        || AFTER_PHRASES.iter().any(|p| after_text.contains(p))
}

/// Whether the span is an example: right after "e.g." or "for example", or
/// inside a parenthetical that starts with one.
fn example(before: &str) -> bool {
    const MARKERS: &[&str] = &[
        "e.g.",
        "e.g.,",
        "eg.",
        "for example",
        "for example,",
        "for instance",
        "for instance,",
    ];
    let lower = before.trim_end().to_lowercase();
    if MARKERS.iter().any(|m| lower.ends_with(m)) {
        return true;
    }
    lower.rfind('(').is_some_and(|open| {
        let inside = lower[open + 1..].trim_start();
        !inside.contains(')') && MARKERS.iter().any(|m| inside.starts_with(m))
    })
}

/// Whether the span is on an unchecked task-list line: `- [ ] Add ...`.
fn todo(before: &str) -> bool {
    let t = before.trim_start();
    ["- [ ]", "* [ ]", "+ [ ]"].iter().any(|m| t.starts_with(m))
}

/// Where clauses end: `.`, `;`, `:`, `!`, or `?` before a space or the end
/// of the line, so `Clock::today` and `../a.md` don't split one.
fn clause_breaks(s: &str) -> impl Iterator<Item = usize> + '_ {
    s.char_indices().filter_map(|(i, c)| {
        let next = s[i + c.len_utf8()..].chars().next();
        (matches!(c, '.' | ';' | ':' | '!' | '?') && next.is_none_or(char::is_whitespace))
            .then_some(i)
    })
}

/// Example names and templates, which aren't claims about this repo.
fn placeholder(code: &str) -> bool {
    if code.contains([
        '<', '>', '{', '}', '*', '$', '%', '=', '[', ']', '"', '\'', '#', '|', '\\', ',', ';', '…',
        '&', '@', '~', '^', '+',
    ]) || code.contains("...")
    {
        return true;
    }
    placeholder_words(code)
}

/// Template words: `NNNN-title.md`, `YYYYMMDD_name.sql`, `foo/bar`.
fn placeholder_words(s: &str) -> bool {
    let lower = s.to_lowercase();
    ["yyyy", "xxx", "nnnn"].iter().any(|p| lower.contains(p))
        || lower.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| {
            matches!(
                w,
                "foo" | "bar" | "baz" | "qux" | "example" | "your" | "my" | "xx" | "yy"
            )
        })
}

const EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "py", "go", "java", "kt", "kts",
    "rb", "swift", "c", "h", "cc", "cpp", "hpp", "m", "mm", "zig", "sql", "md", "mdx", "txt",
    "rst", "toml", "yaml", "yml", "json", "jsonc", "lock", "sh", "bash", "zsh", "fish", "css",
    "scss", "html", "svg", "png", "jpg", "jpeg", "gif", "webp", "pdf", "env", "cfg", "ini", "conf",
    "xml", "proto", "graphql", "gql", "mk", "vue", "svelte", "tf", "hcl", "nix", "vim", "lua",
    "el", "ex", "exs", "erl", "hs", "ml", "php", "pl", "r", "scala", "cs", "fs", "dart", "plist",
    "csv", "tsv", "patch", "diff",
];

/// Whether a code span reads as a file or directory path.
fn looks_like_path(code: &str, first_exists: impl Fn(&str) -> bool) -> bool {
    if code.contains("::")
        || code.contains('(')
        || code.starts_with(['/', '-', '.'])
            && !code.starts_with("./")
            && !code.starts_with("../")
            && !is_dotfile(code)
    {
        return false;
    }
    if !code
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
    {
        return false;
    }
    let trimmed = code.trim_end_matches('/');
    let last = trimmed.rsplit('/').next().unwrap_or(trimmed);
    let has_ext = last
        .rsplit_once('.')
        .is_some_and(|(_, ext)| EXTENSIONS.contains(&ext));
    let has_slash = trimmed.contains('/');
    has_ext
        || (code.ends_with('/') && trimmed.len() > 1)
        || (has_slash && first_exists(trimmed.split('/').next().unwrap_or("")))
        || matches!(
            last,
            "Makefile" | "Dockerfile" | "Justfile" | "justfile" | "Containerfile" | "Gemfile"
        )
}

/// `.env`, `.sqlx/`, `.github/workflows`: a dot-led name, not `.5`, `..`, or
/// an extension like `.sh` or `.gradle.kts`. A dot-led name ending in a
/// programming language's extension is taken as an extension.
fn is_dotfile(code: &str) -> bool {
    const CODE: &[&str] = &[
        "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go", "java", "kt", "kts", "rb",
        "swift", "c", "h", "cc", "cpp", "hpp", "m", "zig", "sh", "lua", "php", "cs", "scala",
    ];
    let Some(rest) = code.strip_prefix('.') else {
        return false;
    };
    let last = rest.rsplit('.').next().unwrap_or(rest);
    let extension = !rest.contains('/')
        && rest
            .split('.')
            .all(|p| p.chars().all(|c| c.is_ascii_alphanumeric()))
        && (CODE.contains(&last)
            || (!rest.contains('.') && EXTENSIONS.contains(&rest) && rest != "env"));
    rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && !extension
}

/// The name a code span claims exists, if it reads as an identifier:
/// `ClientId` from `ClientId(Uuid)`, `today` from `Clock::today()`, `query`
/// from `sqlx::query!`. Plain lowercase words (`strict`, `any`) don't count:
/// in prose they're as likely English as code.
fn symbol_name(code: &str) -> Option<&str> {
    let mut s = code.trim_end_matches(['?', ':', '.']);
    let mut called = false;
    if s.ends_with(')') {
        s = &s[..s.find('(')?];
        called = true;
    }
    if let Some(m) = s.strip_suffix('!') {
        s = m;
        called = true;
    }
    let segments: Vec<&str> = s.split("::").flat_map(|p| p.split('.')).collect();
    let ident = |w: &str| {
        w.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    let (first, rest) = segments.split_first()?;
    // A crate name like `anchor-api` may lead a path.
    let first_ok = ident(first) || (!rest.is_empty() && ident(&first.replace('-', "_")));
    if !first_ok || !rest.iter().all(|w| ident(w)) {
        return None;
    }
    let name = segments.last()?;
    if segments.len() == 1 && !called {
        let camel = name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && name.chars().any(|c| c.is_ascii_lowercase());
        // SCREAMING_SNAKE names are usually environment variables, set
        // outside the code.
        let snake = name.contains('_') && name.chars().any(|c| c.is_ascii_lowercase());
        if !camel && !snake {
            return None;
        }
    }
    // The last segment is what the claim is about. Earlier ones are often
    // other crates' names (`Local::now`), which the repo may never spell out.
    Some(name)
}

/// Targets a Makefile defines: `name:` at the start of a line, not `:=`.
fn targets(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines().flat_map(|line| {
        let names = match line.split_once(':') {
            Some((names, rest))
                if !line.starts_with(['\t', ' ', '#', '.'])
                    && !rest.starts_with('=')
                    && !names.contains(['=', '$', '%']) =>
            {
                names
            }
            _ => "",
        };
        names.split_whitespace().map(str::to_owned)
    })
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Joins `rel` to `dir` and resolves `.` and `..`. `None` if it climbs out
/// of the repo.
fn join(dir: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// `%20` and friends to the characters they stand for.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The candidate nearest `word` by edit distance, if it's close enough to be
/// a likely typo or rename (at most a third of the word changed).
fn closest<'b>(word: &str, candidates: &[&'b str]) -> Option<&'b str> {
    let limit = word.chars().count().div_ceil(3).max(1);
    candidates
        .iter()
        .map(|c| (edit_distance(word, c), *c))
        .filter(|(d, _)| *d <= limit)
        .min()
        .map(|(_, c)| c)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = if ca == *cb {
                prev
            } else {
                1 + prev.min(cur).min(row[j])
            };
            prev = cur;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negation_is_per_clause() {
        assert!(negated("Use `policy_term`, not ", ""));
        assert!(negated("Take a clock rather than calling ", "."));
        assert!(negated("", " was removed in 0.3."));
        assert!(!negated("Not here. Use ", ""));
        assert!(!negated("Use ", " and never look back"));
        assert!(negated("Calls `Clock::today()`, rather than ", ""));
        assert!(!negated("Not here. See ../a.md and ", ""));
        assert!(negated("See ", " once it exists."));
        assert!(negated("", " isn't built yet."));
    }

    #[test]
    fn examples_and_todos_are_not_claims() {
        assert!(example("Loads the template (e.g. "));
        assert!(example("A name, for example "));
        assert!(example("The form (e.g. ACORD 25 or "));
        assert!(!example("(see below) uses "));
        assert!(todo("- [ ] Add the emails to "));
        assert!(!todo("- [x] Added the emails to "));
    }

    #[test]
    fn placeholders_are_not_claims() {
        for p in [
            "Foo.test.tsx",
            "YYYYMMDDHHMMSS_name.sql",
            "<path>",
            "src/*.rs",
            "my-app",
            "Vec<T>",
        ] {
            assert!(placeholder(p), "{p}");
        }
        assert!(!placeholder("lib/money.ts"));
        assert!(!placeholder("ClientId(Uuid)"));
    }

    #[test]
    fn tells_paths_from_other_code() {
        let none = |_: &str| false;
        let crates = |s: &str| s == "crates";
        assert!(looks_like_path("lib/money.ts", none));
        assert!(looks_like_path(".sqlx/", none));
        assert!(looks_like_path(".env", none));
        assert!(looks_like_path("../glossary.md", none));
        assert!(looks_like_path("Cargo.toml", none));
        assert!(looks_like_path("crates/index", crates));
        assert!(!looks_like_path("owner/repo", none));
        assert!(!looks_like_path("term.bind", none));
        assert!(!looks_like_path("/usr/local/bin", none));
        assert!(!looks_like_path("--json", none));
        assert!(!looks_like_path("anchor-api::files", none));
        assert!(!looks_like_path(".sh", none));
        assert!(!looks_like_path(".kts", none));
        assert!(!looks_like_path(".gradle.kts", none));
        assert!(looks_like_path(".github/workflows/ci.yml", none));
        assert!(looks_like_path(".eslintrc.json", none));
    }

    #[test]
    fn picks_the_name_a_symbol_claims() {
        assert_eq!(symbol_name("ClientId(Uuid)"), Some("ClientId"));
        assert_eq!(symbol_name("Clock::today()"), Some("today"));
        assert_eq!(symbol_name("term.bind(at)?"), Some("bind"));
        assert_eq!(symbol_name("sqlx::query!"), Some("query"));
        assert_eq!(
            symbol_name("anchor-api::files::FileStore"),
            Some("FileStore")
        );
        assert_eq!(symbol_name("policy_term"), Some("policy_term"));
        assert_eq!(symbol_name("refresh()"), Some("refresh"));
        assert_eq!(symbol_name("strict"), None);
        assert_eq!(symbol_name("FEIN"), None);
        assert_eq!(symbol_name("JDK_11_0_X64"), None);
        assert_eq!(symbol_name("i64"), None);
        assert_eq!(symbol_name("pre-commit"), None);
        assert_eq!(symbol_name("v1.2"), None);
    }

    #[test]
    fn reads_make_targets() {
        let t: Vec<String> = targets(
            "CC := cc\nall: build\n\tcc\nbuild test: x\n.PHONY: all\n$(OUT): y\n%.o: %.c\n",
        )
        .collect();
        assert_eq!(t, ["all", "build", "test"]);
    }

    #[test]
    fn joins_relative_paths() {
        assert_eq!(
            join("docs/eng", "../glossary.md").as_deref(),
            Some("docs/glossary.md")
        );
        assert_eq!(join("docs", "./a/./b").as_deref(), Some("docs/a/b"));
        assert_eq!(join("docs", "../../x"), None);
    }

    #[test]
    fn suggests_close_names_only() {
        assert_eq!(closest("zzz", &["policy-term", "glossary"]), None);
        assert_eq!(
            closest("policy-period", &["policy-term", "glossary"]),
            Some("policy-term")
        );
        assert_eq!(
            closest("policy-trem", &["policy-term", "glossary"]),
            Some("policy-term")
        );
        assert_eq!(closest("tset", &["test", "lint"]), Some("test"));
    }

    #[test]
    fn history_docs_are_recognized() {
        assert!(is_past_or_future("CHANGELOG.md"));
        assert!(is_past_or_future("docs/decisions/0001-x.md"));
        assert!(is_past_or_future("docs/product/requirements/billing.md"));
        assert!(is_past_or_future("ROADMAP.md"));
        assert!(is_past_or_future("docs/ChangeHistory.md"));
        assert!(is_past_or_future("WhatsNew.md"));
        assert!(!is_past_or_future("docs/design.md"));
    }

    #[test]
    fn fixtures_and_vendored_docs_are_not_ours() {
        let licensed = licensed_dirs(&[
            "LICENSE".into(),
            "lib/bundles/terraform/LICENSE.txt".into(),
            "lib/bundles/terraform/README.md".into(),
        ]);
        let own = |p: &str| is_own_doc(p, &licensed);
        assert!(own("docs/design.md"));
        assert!(own("tests/README.md"));
        assert!(own("lib/bundles/README.md"));
        assert!(!own("lib/bundles/terraform/README.md"));
        assert!(!own("lib/bundles/terraform/docs/a.md"));
        assert!(!own("plugins/markdown/test/data/a.md"));
        assert!(!own("src/__fixtures__/a.md"));
        assert!(!own("vendor/lib/README.md"));
        assert!(!own("src/main.rs"));
    }

    #[test]
    fn scans_identifiers() {
        let words: Vec<&str> = identifiers("let x_1 = Foo::new(2a); été").collect();
        assert_eq!(words, ["let", "x_1", "Foo", "new", "t"]);
    }
}
