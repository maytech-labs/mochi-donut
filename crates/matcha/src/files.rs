//! Reading the repo's tracked text files.

use std::path::Path;

use mochi_index::{IndexError, RepoRoot};

/// Files larger than this are skipped: generated bundles, data, lockfiles.
const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Repo-relative, with `/` separators.
    pub path: String,
    pub text: String,
}

/// Tracked text files under any of `prefixes` (all files if empty). Skips
/// files over 1 MiB and files that look binary (a NUL byte near the start).
pub fn text_files(root: &RepoRoot, prefixes: &[String]) -> Result<Vec<SourceFile>, IndexError> {
    let mut out = Vec::new();
    for path in root.tracked_files()? {
        if !prefixes.is_empty() && !prefixes.iter().any(|p| under(&path, p)) {
            continue;
        }
        let full = root.path().join(&path);
        if !is_small_file(&full) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&full) else {
            continue;
        };
        if bytes.iter().take(8192).any(|&b| b == 0) {
            continue;
        }
        out.push(SourceFile {
            path,
            text: String::from_utf8_lossy(&bytes).into_owned(),
        });
    }
    Ok(out)
}

fn is_small_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() <= MAX_BYTES)
}

/// Whether `path` is `prefix` or inside it. `prefix` may end in `/` or not.
pub fn under(path: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_start_matches("./").trim_end_matches('/');
    prefix.is_empty()
        || path == prefix
        || (path.starts_with(prefix) && path[prefix.len()..].starts_with('/'))
}

/// Lockfiles and minified bundles: generated, so they say nothing about
/// how the repo's people write code, and they swamp counts.
pub fn is_generated(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(
        name,
        "Cargo.lock"
            | "package-lock.json"
            | "npm-shrinkwrap.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "go.sum"
            | "poetry.lock"
            | "uv.lock"
            | "Pipfile.lock"
            | "Gemfile.lock"
            | "composer.lock"
            | "flake.lock"
            | "Package.resolved"
    ) || name.ends_with(".min.js")
        || name.ends_with(".min.css")
}

/// Whether a path is a test: under a `tests`, `test`, `__tests__`, `spec`, or
/// `e2e` directory, or named like `*_test.go`, `*.test.ts`, `*.spec.ts`,
/// `test_*.py`.
pub fn is_test(path: &str) -> bool {
    let mut segs: Vec<&str> = path.split('/').collect();
    let name = segs.pop().unwrap_or("");
    let stem = name.split('.').next().unwrap_or(name);
    segs.iter().any(|s| {
        matches!(
            *s,
            "tests" | "test" | "__tests__" | "spec" | "specs" | "e2e" | "testdata"
        )
    }) || name.contains(".test.")
        || name.contains(".spec.")
        || stem.ends_with("_test")
        || stem.ends_with("_tests")
        || stem.starts_with("test_")
}

/// A Rust file without its inline test module: everything from the first
/// line that starts with `#[cfg(test)]`, which by convention closes the file.
/// Other files come back unchanged.
pub fn without_inline_tests(path: &str, text: &str) -> String {
    if !path.ends_with(".rs") {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// A coarse language label from the file extension, for breakdowns.
pub fn language(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map_or("", |(_, e)| e);
    match ext {
        "rs" => "Rust",
        "ts" | "tsx" | "mts" | "cts" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "py" => "Python",
        "go" => "Go",
        "java" | "kt" | "kts" => "JVM",
        "c" | "h" | "cc" | "cpp" | "hpp" => "C/C++",
        "rb" => "Ruby",
        "swift" => "Swift",
        "sql" => "SQL",
        "md" | "mdx" | "txt" | "rst" => "docs",
        "toml" | "yaml" | "yml" | "json" | "ini" => "config",
        "sh" | "bash" | "zsh" => "shell",
        "css" | "scss" | "html" => "web",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_matching_respects_path_segments() {
        assert!(under("frontend/src/a.ts", "frontend"));
        assert!(under("frontend/src/a.ts", "frontend/"));
        assert!(under("frontend/src/a.ts", "./frontend/src"));
        assert!(!under("frontend-old/a.ts", "frontend"));
        assert!(under("a.ts", ""));
    }

    #[test]
    fn knows_tests_and_generated_files() {
        for t in [
            "backend/tests/contract/a.rs",
            "src/a.test.ts",
            "pkg/a_test.go",
            "test_a.py",
            "frontend/e2e/x.spec.ts",
        ] {
            assert!(is_test(t), "{t}");
        }
        for t in ["src/testing.rs", "src/contest.ts", "docs/test-data.md"] {
            assert!(!is_test(t), "{t}");
        }
        assert_eq!(
            without_inline_tests(
                "a.rs",
                "fn a() {}\n#[cfg(test)]\nmod tests { x.unwrap(); }\n"
            ),
            "fn a() {}\n"
        );
        assert_eq!(
            without_inline_tests("a.ts", "x\n#[cfg(test)]\n"),
            "x\n#[cfg(test)]\n"
        );
        assert!(is_generated("frontend/package-lock.json"));
        assert!(is_generated("Cargo.lock"));
        assert!(!is_generated("Cargo.toml"));
    }

    #[test]
    fn labels_languages() {
        assert_eq!(language("a/b.tsx"), "TypeScript");
        assert_eq!(language("README.md"), "docs");
        assert_eq!(language("Makefile"), "other");
    }
}
