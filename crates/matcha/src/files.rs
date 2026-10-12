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
fn under(path: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_start_matches("./").trim_end_matches('/');
    prefix.is_empty()
        || path == prefix
        || (path.starts_with(prefix) && path[prefix.len()..].starts_with('/'))
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
    fn labels_languages() {
        assert_eq!(language("a/b.tsx"), "TypeScript");
        assert_eq!(language("README.md"), "docs");
        assert_eq!(language("Makefile"), "other");
    }
}
