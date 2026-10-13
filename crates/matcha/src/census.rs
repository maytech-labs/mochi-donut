//! `matcha census`: how does this repo usually do X? Counts each alternative
//! across tracked files, by language, with the most typical examples
//! (docs/design.md, "census").

use std::collections::BTreeMap;

use regex::Regex;
use serde::Serialize;

use crate::files::{SourceFile, is_test, language};

/// One alternative's tally.
#[derive(Debug, Serialize)]
pub struct Tally {
    pub pattern: String,
    /// Files with at least one match.
    pub files: usize,
    pub matches: usize,
    /// How many of `files` are tests.
    pub test_files: usize,
    /// This alternative's share of all matching files, 0 to 1.
    pub share: f64,
    /// The pattern's core word, when it alone appears in more files than the
    /// pattern does, so a too-narrow regex (`useForm\(` against
    /// `useForm<T>(`) doesn't read as "never used".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub near_miss: Option<NearMiss>,
    pub by_language: BTreeMap<&'static str, usize>,
    pub examples: Vec<Example>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NearMiss {
    pub word: String,
    pub files: usize,
    pub example: Example,
}

#[derive(Debug, Clone, Serialize)]
pub struct Example {
    pub path: String,
    /// First matching line, 1-based.
    pub line: usize,
    pub matches: usize,
}

#[derive(Debug, Serialize)]
pub struct Census {
    pub query: Option<String>,
    pub files_searched: usize,
    pub alternatives: Vec<Tally>,
}

/// Counts each pattern in `files`. Examples are the files with the most
/// matches, shorter paths first on ties.
pub fn alternatives(
    query: Option<String>,
    patterns: &[Regex],
    files: &[SourceFile],
    examples: usize,
) -> Census {
    let mut tallies: Vec<Tally> = patterns
        .iter()
        .map(|re| tally(re, files, examples))
        .collect();
    let total: usize = tallies.iter().map(|t| t.files).sum();
    for t in &mut tallies {
        t.share = if total == 0 {
            0.0
        } else {
            t.files as f64 / total as f64
        };
    }
    Census {
        query,
        files_searched: files.len(),
        alternatives: tallies,
    }
}

fn tally(re: &Regex, files: &[SourceFile], examples: usize) -> Tally {
    let mut hits: Vec<Example> = Vec::new();
    let mut by_language: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut matches = 0;
    for file in files {
        let mut count = 0;
        let mut first_line = 0;
        for (i, line) in file.text.lines().enumerate() {
            let n = re.find_iter(line).count();
            if n > 0 && first_line == 0 {
                first_line = i + 1;
            }
            count += n;
        }
        if count > 0 {
            matches += count;
            *by_language.entry(language(&file.path)).or_insert(0) += count;
            hits.push(Example {
                path: file.path.clone(),
                line: first_line,
                matches: count,
            });
        }
    }
    let files_matched = hits.len();
    let test_files = hits.iter().filter(|h| is_test(&h.path)).count();
    hits.sort_by(|a, b| {
        b.matches
            .cmp(&a.matches)
            .then_with(|| a.path.len().cmp(&b.path.len()))
            .then_with(|| a.path.cmp(&b.path))
    });
    hits.truncate(examples);
    Tally {
        pattern: re.as_str().to_owned(),
        files: files_matched,
        matches,
        test_files,
        share: 0.0,
        near_miss: near_miss(re, files).filter(|m| m.files > files_matched),
        by_language,
        examples: hits,
    }
}

/// The longest literal word in a regex (`useForm` in `useForm\(`), searched
/// on its own.
fn near_miss(re: &Regex, files: &[SourceFile]) -> Option<NearMiss> {
    let word = core_word(re.as_str())?;
    let word_re = Regex::new(&format!(r"\b{}\b", regex::escape(&word))).ok()?;
    let mut hits = files.iter().filter_map(|f| {
        f.text
            .lines()
            .position(|l| word_re.is_match(l))
            .map(|i| (f, i + 1))
    });
    let (first, line) = hits.next()?;
    Some(NearMiss {
        word,
        files: 1 + hits.count(),
        example: Example {
            path: first.path.clone(),
            line,
            matches: 1,
        },
    })
}

/// The longest run of identifier characters in a pattern that isn't a
/// regex escape, if it's at least 4 long.
fn core_word(pattern: &str) -> Option<String> {
    let mut best = String::new();
    let mut cur = String::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            // `\w`, `\b`, `\(`: an escape ends the word and isn't part of one.
            chars.next();
            if cur.len() > best.len() {
                best = std::mem::take(&mut cur);
            }
            cur.clear();
        } else if c.is_ascii_alphanumeric() || c == '_' {
            cur.push(c);
        } else {
            if cur.len() > best.len() {
                best = std::mem::take(&mut cur);
            }
            cur.clear();
        }
    }
    if cur.len() > best.len() {
        best = cur;
    }
    (best.len() >= 4).then_some(best)
}

/// One spelling of a word and where it appears.
#[derive(Debug, Serialize)]
pub struct Variant {
    pub word: String,
    pub count: usize,
    pub by_language: BTreeMap<&'static str, usize>,
    pub example: Example,
}

#[derive(Debug, Serialize)]
pub struct Spelling {
    pub stem: String,
    pub files_searched: usize,
    pub variants: Vec<Variant>,
}

/// Every word that starts with `stem`, case-insensitively, across
/// identifiers (split at `_` and camelCase), strings, and prose.
pub fn spelling(stem: &str, files: &[SourceFile]) -> Spelling {
    let stem_lower = stem.to_lowercase();
    let mut variants: BTreeMap<String, Variant> = BTreeMap::new();
    for file in files {
        for (i, line) in file.text.lines().enumerate() {
            for word in words(line) {
                if !word.starts_with(&stem_lower) {
                    continue;
                }
                let v = variants.entry(word.clone()).or_insert_with(|| Variant {
                    word,
                    count: 0,
                    by_language: BTreeMap::new(),
                    example: Example {
                        path: file.path.clone(),
                        line: i + 1,
                        matches: 0,
                    },
                });
                v.count += 1;
                *v.by_language.entry(language(&file.path)).or_insert(0) += 1;
            }
        }
    }
    let mut variants: Vec<Variant> = variants.into_values().collect();
    variants.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.word.cmp(&b.word)));
    Spelling {
        stem: stem.to_owned(),
        files_searched: files.len(),
        variants,
    }
}

/// Lowercased words in `line`: runs of ASCII letters, split at camelCase
/// boundaries (`isCancelled` gives `is`, `cancelled`).
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for c in line.chars() {
        if c.is_ascii_alphabetic() {
            if c.is_ascii_uppercase() && prev_lower && !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            current.push(c.to_ascii_lowercase());
            prev_lower = c.is_ascii_lowercase();
        } else {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            prev_lower = false;
        }
    }
    if !current.is_empty() {
        out.push(current);
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
    fn splits_identifiers_into_words() {
        assert_eq!(
            words("isCancelled for_cancellation CANCELED x2y"),
            [
                "is",
                "cancelled",
                "for",
                "cancellation",
                "canceled",
                "x",
                "y"
            ]
        );
    }

    #[test]
    fn counts_each_alternative_with_examples() {
        let files = [
            file("a/Form.tsx", "useState<Draft>\nuseState<Draft>\n"),
            file("b/Other.tsx", "const x = 1;\nuseState<Draft>\n"),
            file("c/Client.tsx", "useForm()\n"),
        ];
        let patterns = [
            Regex::new(r"useState<\w*Draft>").unwrap(),
            Regex::new(r"useForm\(").unwrap(),
        ];
        let c = alternatives(Some("forms".into()), &patterns, &files, 1);
        assert_eq!(c.files_searched, 3);
        let draft = &c.alternatives[0];
        assert_eq!((draft.files, draft.matches), (2, 3));
        assert!((draft.share - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(draft.by_language["TypeScript"], 3);
        assert_eq!(draft.examples.len(), 1);
        assert_eq!(draft.examples[0].path, "a/Form.tsx");
        assert_eq!(draft.examples[0].line, 1);
        let other = &c.alternatives[1];
        assert_eq!(other.examples[0].path, "c/Client.tsx");
    }

    #[test]
    fn a_pattern_that_matches_nothing_points_at_near_misses() {
        let files = [file(
            "src/ClientForm.tsx",
            "import x;\nconst f = useForm<Values>({});\n",
        )];
        let patterns = [Regex::new(r"useForm\(").unwrap()];
        let c = alternatives(None, &patterns, &files, 3);
        let miss = c.alternatives[0].near_miss.as_ref().unwrap();
        assert_eq!((miss.word.as_str(), miss.files), ("useForm", 1));
        assert_eq!(
            (miss.example.path.as_str(), miss.example.line),
            ("src/ClientForm.tsx", 2)
        );
        assert_eq!(
            core_word(r"\bTermStatus::Cancel+ed\b").as_deref(),
            Some("TermStatus")
        );
        assert_eq!(core_word(r"\w+\("), None);
    }

    #[test]
    fn finds_spelling_variants_by_language() {
        let files = [
            file("src/a.rs", "TermStatus::Cancelled // cancelled\n"),
            file("docs/b.md", "A canceled term.\n"),
            file("db/c.sql", "status = 'cancelled'\n"),
        ];
        let s = spelling("cancel", &files);
        assert_eq!(s.variants[0].word, "cancelled");
        assert_eq!(s.variants[0].count, 3);
        assert_eq!(s.variants[0].by_language["Rust"], 2);
        assert_eq!(s.variants[0].by_language["SQL"], 1);
        assert_eq!(s.variants[1].word, "canceled");
        assert_eq!(s.variants[1].example.path, "docs/b.md");
    }
}
