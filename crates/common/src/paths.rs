//! What kind of file a path is, shared by matcha's census and ube's done.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_test_paths() {
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
    }
}
