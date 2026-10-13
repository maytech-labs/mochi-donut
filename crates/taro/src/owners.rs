//! `taro owners`: who owns these files, and who has changed them most. From
//! CODEOWNERS and the local history; no network (docs/taro/design.md,
//! "taro owners").

use std::collections::BTreeMap;
use std::path::Path;

use regex::Regex;
use serde::Serialize;

use crate::forge::git;

/// One CODEOWNERS rule.
#[derive(Debug, Clone)]
pub struct Rule {
    pattern: Regex,
    pub owners: Vec<String>,
}

/// The CODEOWNERS file's rules, from `.github/`, the root, or `docs/`.
pub fn codeowners(root: &Path) -> (Option<String>, Vec<Rule>) {
    for rel in [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"] {
        if let Ok(text) = std::fs::read_to_string(root.join(rel)) {
            return (Some(rel.to_owned()), parse(&text));
        }
    }
    (None, Vec::new())
}

pub fn parse(text: &str) -> Vec<Rule> {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .filter_map(|l| {
            let mut words = l.split_whitespace();
            let pattern = glob(words.next()?)?;
            Some(Rule {
                pattern,
                owners: words.map(str::to_owned).collect(),
            })
        })
        .collect()
}

/// A CODEOWNERS pattern as a regex over repo-relative paths, following
/// gitignore rules: a leading `/` anchors it, a trailing `/` matches a
/// directory's contents, a name without `/` matches at any depth.
fn glob(p: &str) -> Option<Regex> {
    let anchored = p.starts_with('/') || p.trim_end_matches('/').contains('/');
    let dir = p.ends_with('/');
    let core = p.trim_start_matches('/').trim_end_matches('/');
    let mut re = String::new();
    let mut chars = core.chars().peekable();
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
    let prefix = if anchored { "^" } else { "^(?:.*/)?" };
    // A name or directory matches everything under it too.
    let suffix = if dir { "/.*$" } else { "(?:/.*)?$" };
    Regex::new(&format!("{prefix}{re}{suffix}")).ok()
}

/// The owners of `path`: the last matching rule's.
pub fn owners_of<'a>(rules: &'a [Rule], path: &str) -> Option<&'a [String]> {
    rules
        .iter()
        .rev()
        .find(|r| r.pattern.is_match(path))
        .map(|r| r.owners.as_slice())
}

#[derive(Debug, Serialize)]
pub struct Owners {
    pub codeowners_file: Option<String>,
    pub paths: Vec<PathOwners>,
    /// Who has changed these paths most in the window, with commit counts.
    pub frequent: Vec<(String, usize)>,
    /// CODEOWNERS owners, then frequent authors, without you.
    pub suggested: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PathOwners {
    pub path: String,
    pub owners: Vec<String>,
}

pub fn owners(root: &Path, paths: &[String], since: &str) -> Result<Owners, String> {
    let (file, rules) = codeowners(root);
    let per_path: Vec<PathOwners> = paths
        .iter()
        .map(|p| PathOwners {
            path: p.clone(),
            owners: owners_of(&rules, p)
                .map(<[String]>::to_vec)
                .unwrap_or_default(),
        })
        .collect();
    let mut args = vec!["log", "--no-merges", "--format=%an <%ae>"];
    let since_arg = format!("--since={since}");
    args.push(&since_arg);
    args.push("--");
    args.extend(paths.iter().map(String::as_str));
    let log = git(root, &args)?;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for author in log.lines().filter(|l| !l.is_empty()) {
        *counts.entry(author.to_owned()).or_insert(0) += 1;
    }
    let mut frequent: Vec<(String, usize)> = counts.into_iter().collect();
    frequent.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    frequent.truncate(5);
    let me = git(root, &["config", "user.email"]).unwrap_or_default();
    let me = me.trim();
    let mut suggested: Vec<String> = Vec::new();
    for o in per_path.iter().flat_map(|p| p.owners.iter()) {
        if !suggested.contains(o) {
            suggested.push(o.clone());
        }
    }
    for (a, _) in &frequent {
        if (me.is_empty() || !a.contains(me)) && !suggested.contains(a) {
            suggested.push(a.clone());
        }
    }
    suggested.truncate(5);
    Ok(Owners {
        codeowners_file: file,
        paths: per_path,
        frequent,
        suggested,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_codeowners_rules() {
        let rules = parse(
            "# owners\n* @team/all\n*.md @docs\n/crates/yuzu/ @me @you\ndocs/**/design.md @arch\napps/web @web # trailing\n",
        );
        let o = |p: &str| {
            owners_of(&rules, p)
                .map(|o| o.join(" "))
                .unwrap_or_default()
        };
        assert_eq!(o("Cargo.toml"), "@team/all");
        assert_eq!(o("crates/matcha/README.md"), "@docs");
        assert_eq!(o("crates/yuzu/src/wait.rs"), "@me @you");
        assert_eq!(o("docs/ube/design.md"), "@arch");
        assert_eq!(o("apps/web/src/a.ts"), "@web");
        assert_eq!(o("x/crates/yuzu/a.rs"), "@team/all");
    }
}
