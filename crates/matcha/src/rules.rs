//! Rule claims for `drift`: a check comment beside a sentence, holding the
//! check that proves it (docs/design.md, "drift";
//! docs/decisions/0016-drift-check-comments.md).
//!
//! ```markdown
//! <!-- matcha: no-match 'Utc::now\(' except 'clock.rs' -->
//! Never call `Utc::now()` outside a `Clock` implementation.
//! ```

use regex::Regex;
use serde::Serialize;

use crate::files::{SourceFile, under};

/// What a check comment asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Expect {
    /// The pattern must match nowhere.
    NoMatch,
    /// The pattern must match somewhere.
    Match,
}

#[derive(Debug)]
pub struct Rule {
    pub expect: Expect,
    pub pattern: Regex,
    /// Paths or globs to search. Empty means every source file.
    pub within: Vec<String>,
    /// Paths or globs to leave out.
    pub except: Vec<String>,
}

pub const SYNTAX: &str = "Write it as <!-- matcha: no-match '<regex>' [in '<path>'] [except '<path or glob>'] --> or <!-- matcha: match '<regex>' ... -->.";

/// Reads a check comment's text (after `matcha:`).
pub fn parse(text: &str) -> Result<Rule, String> {
    let words = tokens(text)?;
    let mut it = words.into_iter();
    let expect = match it.next().as_deref() {
        Some("no-match") => Expect::NoMatch,
        Some("match") => Expect::Match,
        Some(other) => return Err(format!("unknown check `{other}`")),
        None => return Err("the comment is empty".into()),
    };
    let pattern = it.next().ok_or("the check has no pattern after it")?;
    let pattern =
        Regex::new(&pattern).map_err(|e| format!("`{pattern}` isn't a valid regex: {e}"))?;
    let mut rule = Rule {
        expect,
        pattern,
        within: Vec::new(),
        except: Vec::new(),
    };
    while let Some(word) = it.next() {
        let list = match word.as_str() {
            "in" => &mut rule.within,
            "except" => &mut rule.except,
            other => return Err(format!("expected `in` or `except`, found `{other}`")),
        };
        list.push(it.next().ok_or(format!("`{word}` needs a path after it"))?);
    }
    Ok(rule)
}

/// Splits on whitespace, keeping '...' and "..." together. Inside quotes,
/// only `\'` (or `\"`) is an escape, so regexes keep their backslashes.
fn tokens(text: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let mut word = String::new();
        if c == '\'' || c == '"' {
            chars.next();
            let mut closed = false;
            while let Some(ch) = chars.next() {
                if ch == '\\' && chars.peek() == Some(&c) {
                    word.push(c);
                    chars.next();
                } else if ch == c {
                    closed = true;
                    break;
                } else {
                    word.push(ch);
                }
            }
            if !closed {
                return Err(format!("a {c} quote isn't closed"));
            }
        } else {
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                word.push(ch);
                chars.next();
            }
        }
        out.push(word);
    }
    Ok(out)
}

/// Whether `path` is named by `spec`: a directory or file (`backend`,
/// `src/clock.rs`), a file name at any depth (`clock.rs`), or a glob
/// (`**/*_test.go`, `*.sql`).
pub fn path_matches(spec: &str, path: &str) -> bool {
    let spec = spec.trim_start_matches("./");
    if spec.contains(['*', '?']) {
        let target = if spec.contains('/') {
            path
        } else {
            path.rsplit('/').next().unwrap_or(path)
        };
        return glob(spec).is_some_and(|re| re.is_match(target));
    }
    under(path, spec)
        || path.rsplit('/').next() == Some(spec)
        || path.ends_with(&format!("/{spec}"))
}

fn glob(spec: &str) -> Option<Regex> {
    let mut re = String::from("^");
    let mut chars = spec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    chars.next();
                    re.push_str("(?:.*/)?");
                } else {
                    re.push_str(".*");
                }
            }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
    }
    re.push('$');
    Regex::new(&re).ok()
}

/// One place a pattern matched.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub path: String,
    pub line: usize,
}

#[derive(Debug, Serialize)]
pub struct Evaluation {
    pub holds: bool,
    pub files_searched: usize,
    /// Matching lines, up to the first 1000.
    pub hits: Vec<Hit>,
    pub files_matched: usize,
}

/// Runs `rule` over `files`. With no `in`, only source files (not docs) are
/// searched, so the sentence the rule sits beside doesn't match itself.
pub fn evaluate(rule: &Rule, files: &[SourceFile], is_source: impl Fn(&str) -> bool) -> Evaluation {
    let mut hits = Vec::new();
    let mut files_searched = 0;
    let mut files_matched = 0;
    for file in files {
        let wanted = if rule.within.is_empty() {
            is_source(&file.path)
        } else {
            rule.within.iter().any(|w| path_matches(w, &file.path))
        };
        if !wanted || rule.except.iter().any(|e| path_matches(e, &file.path)) {
            continue;
        }
        files_searched += 1;
        let mut matched = false;
        for (i, line) in file.text.lines().enumerate() {
            if rule.pattern.is_match(line) {
                matched = true;
                if hits.len() < 1000 {
                    hits.push(Hit {
                        path: file.path.clone(),
                        line: i + 1,
                    });
                }
            }
        }
        files_matched += usize::from(matched);
    }
    let holds = match rule.expect {
        Expect::NoMatch => hits.is_empty(),
        Expect::Match => !hits.is_empty(),
    };
    Evaluation {
        holds,
        files_searched,
        hits,
        files_matched,
    }
}

/// `3 lines in 2 files (a.rs:10, b.rs:4, b.rs:9)`.
pub fn describe_hits(e: &Evaluation) -> String {
    let examples: Vec<String> = e
        .hits
        .iter()
        .take(3)
        .map(|h| format!("{}:{}", h.path, h.line))
        .collect();
    let lines = if e.hits.len() >= 1000 {
        "1000+".to_owned()
    } else {
        e.hits.len().to_string()
    };
    format!(
        "{lines} {} in {} {} ({})",
        plural(e.hits.len(), "line"),
        e.files_matched,
        plural(e.files_matched, "file"),
        examples.join(", ")
    )
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        word.to_owned()
    } else {
        format!("{word}s")
    }
}

/// A check comment proposed for a sentence that reads like a rule.
#[derive(Debug, Serialize)]
pub struct Suggestion {
    pub doc: String,
    pub line: usize,
    pub sentence: String,
    pub comment: String,
    /// Whether the code follows the sentence today.
    pub holds_now: bool,
    pub evidence: String,
}

/// Proposes a check comment for a line, if one of its sentences reads like
/// a rule and names an identifier after the rule word: "Never call `X`"
/// gives `no-match 'X'`; "Every handler must call `Y`" gives `match 'Y'`. A
/// path after "outside" or "except" becomes `except`. `is_name` says which
/// code spans are identifiers worth searching for.
pub fn suggest(line: &str, is_name: impl Fn(&str) -> bool) -> Option<String> {
    const NEGATIVE: &[&str] = &["never ", "don't ", "do not ", "must not ", "avoid "];
    const POSITIVE: &[&str] = &["always ", "every ", "must "];
    for sentence in sentences(line) {
        let lower = sentence.to_lowercase();
        let find = |words: &[&str]| {
            words
                .iter()
                .filter_map(|w| {
                    lower
                        .match_indices(w)
                        .find(|(i, _)| *i == 0 || !lower.as_bytes()[i - 1].is_ascii_alphanumeric())
                        .map(|(i, _)| i)
                })
                .min()
        };
        // "No `x`" opens a rule; "has no `x`" mid-sentence usually doesn't.
        let opening = lower.trim_start_matches(|c: char| {
            c.is_whitespace() || c.is_ascii_digit() || matches!(c, '-' | '*' | '+' | '.' | '>')
        });
        let no_at = opening
            .starts_with("no ")
            .then(|| lower.len() - opening.len());
        let negative = [find(NEGATIVE), no_at].into_iter().flatten().min();
        let (expect, at) = match (negative, find(POSITIVE)) {
            (Some(n), _) => ("no-match", n),
            (None, Some(p)) => ("match", p),
            (None, None) => continue,
        };
        let spans = crate::markdown::spans_in(sentence);
        let Some((_, _, target)) = spans
            .iter()
            .find(|(start, _, code)| *start > at && is_name(code) && !code.contains(['\'', '"']))
        else {
            continue;
        };
        let mut comment = format!("{expect} '{}'", regex::escape(target));
        let except_at = ["outside", "except"]
            .iter()
            .filter_map(|w| lower.find(w))
            .min();
        if let Some(e) = except_at
            && let Some((_, _, path)) = spans
                .iter()
                .find(|(start, _, code)| *start > e && is_path_like(code))
        {
            comment.push_str(&format!(" except '{path}'"));
        }
        return Some(format!("<!-- matcha: {comment} -->"));
    }
    None
}

fn is_path_like(s: &str) -> bool {
    s.contains('/')
        || s.rsplit_once('.').is_some_and(|(stem, ext)| {
            !stem.is_empty()
                && (1..=4).contains(&ext.len())
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
        })
}

/// A line split at `. `, `; `, `! `, and `? `.
fn sentences(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = line.as_bytes();
    for i in 0..bytes.len() {
        if matches!(bytes[i], b'.' | b';' | b'!' | b'?')
            && bytes.get(i + 1).is_none_or(|b| b.is_ascii_whitespace())
        {
            out.push(&line[start..=i]);
            start = i + 1;
        }
    }
    if start < line.len() {
        out.push(&line[start..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, text: &str) -> SourceFile {
        SourceFile {
            path: path.into(),
            text: text.into(),
        }
    }

    #[test]
    fn parses_check_comments() {
        let r = parse(r"no-match 'Utc::now\(|Local::now\(' except 'clock.rs' except '**/tests/**'")
            .unwrap();
        assert_eq!(r.expect, Expect::NoMatch);
        assert_eq!(r.pattern.as_str(), r"Utc::now\(|Local::now\(");
        assert_eq!(r.except, ["clock.rs", "**/tests/**"]);
        let r = parse(r#"match "it's" in 'src'"#).unwrap();
        assert_eq!(r.expect, Expect::Match);
        assert_eq!(r.within, ["src"]);
        assert_eq!(parse(r"match 'a\'b'").unwrap().pattern.as_str(), "a'b");
    }

    #[test]
    fn bad_comments_say_what_is_wrong() {
        assert!(parse("forbid 'x'").unwrap_err().contains("unknown check"));
        assert!(parse("no-match").unwrap_err().contains("no pattern"));
        assert!(parse("no-match '(' ").unwrap_err().contains("valid regex"));
        assert!(parse("no-match 'x").unwrap_err().contains("isn't closed"));
        assert!(
            parse("no-match 'x' within 'y'")
                .unwrap_err()
                .contains("expected")
        );
        assert!(
            parse("no-match 'x' except")
                .unwrap_err()
                .contains("needs a path")
        );
    }

    #[test]
    fn matches_paths_names_and_globs() {
        assert!(path_matches("clock.rs", "backend/src/clock.rs"));
        assert!(path_matches("backend", "backend/src/clock.rs"));
        assert!(path_matches("src/clock.rs", "backend/src/clock.rs"));
        assert!(!path_matches("lock.rs", "backend/src/clock.rs"));
        assert!(path_matches("*.sql", "db/migrations/1.sql"));
        assert!(path_matches("**/tests/**", "crates/a/tests/x.rs"));
        assert!(path_matches("crates/*/src/*.rs", "crates/a/src/x.rs"));
        assert!(!path_matches("crates/*/src/*.rs", "crates/a/src/deep/x.rs"));
    }

    #[test]
    fn evaluates_against_source_files_only_by_default() {
        let files = [
            file("src/clock.rs", "Utc::now()\n"),
            file("src/a.rs", "fn f() {}\nlet t = Utc::now();\n"),
            file("docs/rules.md", "Never call `Utc::now()`.\n"),
        ];
        let is_source = |p: &str| !p.ends_with(".md");
        let rule = parse(r"no-match 'Utc::now\(' except 'clock.rs'").unwrap();
        let e = evaluate(&rule, &files, is_source);
        assert!(!e.holds);
        assert_eq!(e.files_searched, 1);
        assert_eq!(describe_hits(&e), "1 line in 1 file (src/a.rs:2)");

        let rule = parse(r"no-match 'Utc::now\(' except 'src'").unwrap();
        assert!(evaluate(&rule, &files, is_source).holds);
        let rule = parse(r"match 'fn f'").unwrap();
        assert!(evaluate(&rule, &files, is_source).holds);
    }

    #[test]
    fn suggests_comments_for_rule_sentences() {
        let name = |s: &str| {
            !is_path_like(s)
                && s.chars()
                    .any(|c| c.is_ascii_uppercase() || c == '_' || c == '(')
        };
        assert_eq!(
            suggest("Never call `Utc::now()` outside `clock.rs`.", name).as_deref(),
            Some(r"<!-- matcha: no-match 'Utc::now\(\)' except 'clock.rs' -->")
        );
        assert_eq!(
            suggest("Every route must call `authorize()`.", name).as_deref(),
            Some(r"<!-- matcha: match 'authorize\(\)' -->")
        );
        assert_eq!(
            suggest("- **No `unwrap()`** outside tests.", name).as_deref(),
            Some(r"<!-- matcha: no-match 'unwrap\(\)' -->")
        );
        assert_eq!(suggest("We use `Cents` for money.", name), None);
        assert_eq!(suggest("Never edit `schema.d.ts` by hand.", name), None);
        // The rule word must come before the name, in the same sentence.
        assert_eq!(suggest("Money is `Cents`, never floats.", name), None);
        assert_eq!(suggest("Use `Cents`. Never round by hand.", name), None);
        // "no" counts only at the start of a sentence.
        assert_eq!(
            suggest("IDs that have no `Proves_Tag` are listed.", name),
            None
        );
    }
}
