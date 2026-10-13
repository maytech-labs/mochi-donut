//! Failures from check output, a few lines each: cargo test, build, and
//! clippy; pytest; go test; vitest and jest; tsc; eslint; and a generic
//! `path:line: error` fallback (docs/ube/design.md, "ube run").

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    /// The failing test, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub message: String,
    /// A command that reruns just this failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerun: Option<String>,
}

/// At most this many failures are kept per check.
const MAX: usize = 30;

/// Failures in `output` from running `command`.
pub fn failures(command: &str, output: &str) -> Vec<Failure> {
    let mut out = if command.contains("cargo") || command.contains("xtask") {
        let mut v = cargo_tests(command, output);
        v.extend(rust_diagnostics(output));
        v
    } else if command.contains("pytest") {
        pytest(output)
    } else if command.starts_with("go ") {
        go_test(output)
    } else if output.contains("● ") {
        jest(output)
    } else if output.contains("FAIL ")
        && (command.contains("vitest") || output.contains("vitest") || output.contains(" > "))
    {
        vitest(output)
    } else {
        Vec::new()
    };
    if out.is_empty() {
        out.extend(tsc(output));
        out.extend(eslint(output));
    }
    if out.is_empty() {
        out = generic(output);
    }
    out.truncate(MAX);
    out
}

fn re(pattern: &str) -> Regex {
    // The patterns are literals in this file; a bad one is caught by tests.
    Regex::new(pattern).unwrap_or_else(|_| Regex::new("$^").unwrap_or_else(|_| unreachable!()))
}

fn package(command: &str) -> Option<&str> {
    let words: Vec<&str> = command.split_whitespace().collect();
    words
        .iter()
        .position(|w| *w == "-p" || *w == "--package")
        .and_then(|i| words.get(i + 1).copied())
}

/// `---- name stdout ----` blocks, with the panic's location and message.
fn cargo_tests(command: &str, output: &str) -> Vec<Failure> {
    let header = re(r"^---- (\S+) stdout ----$");
    let panic = re(r"panicked at ([^:\s]+):(\d+):\d+:?$");
    let pkg = package(command)
        .map(|p| format!(" -p {p}"))
        .unwrap_or_default();
    let mut out = Vec::new();
    let lines: Vec<&str> = output.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let Some(cap) = header.captures(lines[i]) else {
            i += 1;
            continue;
        };
        let name = cap[1].to_owned();
        let mut f = Failure {
            test: Some(name.clone()),
            file: None,
            line: None,
            message: String::new(),
            rerun: Some(format!("cargo test{pkg} -- {name} --exact")),
        };
        i += 1;
        let mut message: Vec<&str> = Vec::new();
        while i < lines.len() && !header.is_match(lines[i]) && !lines[i].starts_with("failures:") {
            let l = lines[i];
            if let Some(c) = panic.captures(l) {
                f.file = Some(c[1].to_owned());
                f.line = c[2].parse().ok();
            } else if !l.trim().is_empty()
                && !l.starts_with("note: ")
                && !l.starts_with("stack backtrace")
                && message.len() < 6
            {
                message.push(l.trim_end());
            }
            i += 1;
        }
        f.message = message.join("\n");
        out.push(f);
    }
    out
}

/// `error[E0425]: msg` or `error: msg` followed by ` --> file:line:col`.
fn rust_diagnostics(output: &str) -> Vec<Failure> {
    let head = re(r"^error(\[E\d+\])?: (.+)$");
    let at = re(r"^\s*--> ([^:\s]+):(\d+):\d+");
    let lines: Vec<&str> = output.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let Some(c) = head.captures(l) else { continue };
        let message = c[2].to_owned();
        if message.starts_with("could not compile")
            || message.starts_with("aborting")
            || message.starts_with("test failed")
        {
            continue;
        }
        let loc = lines[i + 1..].iter().take(3).find_map(|n| at.captures(n));
        out.push(Failure {
            test: None,
            file: loc.as_ref().map(|c| c[1].to_owned()),
            line: loc.as_ref().and_then(|c| c[2].parse().ok()),
            message,
            rerun: None,
        });
    }
    out
}

/// pytest's summary: `FAILED path::test - message` and `ERROR path - message`.
fn pytest(output: &str) -> Vec<Failure> {
    let line = re(r"^(FAILED|ERROR) (\S+?)(?:::(\S+))?(?: - (.+))?$");
    output
        .lines()
        .filter_map(|l| line.captures(l))
        .map(|c| {
            let file = c[2].to_owned();
            let test = c.get(3).map(|m| m.as_str().to_owned());
            let id = match &test {
                Some(t) => format!("{file}::{t}"),
                None => file.clone(),
            };
            Failure {
                test,
                file: Some(file),
                line: None,
                message: c
                    .get(4)
                    .map_or_else(|| c[1].to_lowercase(), |m| m.as_str().to_owned()),
                rerun: Some(format!("python -m pytest '{id}'")),
            }
        })
        .collect()
}

/// `--- FAIL: TestX`, then indented `file_test.go:12: message` lines; the
/// package from the `FAIL\tpkg` line after.
fn go_test(output: &str) -> Vec<Failure> {
    let fail = re(r"^\s*--- FAIL: (\S+)");
    let at = re(r"^\s+([\w./-]+\.go):(\d+): (.+)$");
    let pkg_line = re(r"^FAIL\s+(\S+)\s");
    let lines: Vec<&str> = output.lines().collect();
    let mut out: Vec<Failure> = Vec::new();
    let mut pending: Vec<usize> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if let Some(c) = fail.captures(l) {
            let loc = lines[i + 1..].iter().take(5).find_map(|n| at.captures(n));
            out.push(Failure {
                test: Some(c[1].to_owned()),
                file: loc.as_ref().map(|c| c[1].to_owned()),
                line: loc.as_ref().and_then(|c| c[2].parse().ok()),
                message: loc
                    .as_ref()
                    .map_or_else(|| "failed".to_owned(), |c| c[3].to_owned()),
                rerun: None,
            });
            pending.push(out.len() - 1);
        } else if let Some(c) = pkg_line.captures(l) {
            for j in pending.drain(..) {
                let name = out[j].test.clone().unwrap_or_default();
                let top = name.split('/').next().unwrap_or(&name).to_owned();
                out[j].rerun = Some(format!("go test {} -run '^{top}$'", &c[1]));
            }
        }
    }
    out
}

/// vitest: `FAIL  file > suite > test`, then the error and `❯ file:line`.
fn vitest(output: &str) -> Vec<Failure> {
    let fail = re(r"^\s*(?:×|FAIL)\s+(\S+\.\w+)\s+>\s+(.+?)\s*(?:\d+ms)?$");
    let at = re(r"❯ ([^\s:]+):(\d+):\d+");
    let lines: Vec<&str> = output.lines().collect();
    let mut out: Vec<Failure> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let Some(c) = fail.captures(l) else { continue };
        let file = c[1].to_owned();
        let name = c[2].to_owned();
        if out
            .iter()
            .any(|f| f.file.as_deref() == Some(&file) && f.test.as_deref() == Some(&name))
        {
            continue;
        }
        let following = &lines[i + 1..lines.len().min(i + 12)];
        let message = following
            .iter()
            .map(|n| n.trim())
            .find(|n| n.contains("Error") || n.starts_with("expected") || n.starts_with("Expected"))
            .unwrap_or("failed")
            .to_owned();
        let loc = following.iter().find_map(|n| at.captures(n));
        let test = name.rsplit(" > ").next().unwrap_or(&name).to_owned();
        out.push(Failure {
            rerun: Some(format!("npx vitest run {file} -t \"{test}\"")),
            test: Some(name),
            line: loc.as_ref().and_then(|c| c[2].parse().ok()),
            file: Some(file),
            message,
        });
    }
    out
}

/// jest: `● suite › test`, then the message and `at ... (file:line:col)`.
fn jest(output: &str) -> Vec<Failure> {
    let head = re(r"^\s*● (.+)$");
    let at = re(r"\(([^\s()]+\.\w+):(\d+):\d+\)");
    let file_line = re(r"^FAIL\s+(\S+)");
    let lines: Vec<&str> = output.lines().collect();
    let mut out = Vec::new();
    let mut current_file: Option<String> = None;
    for (i, l) in lines.iter().enumerate() {
        if let Some(c) = file_line.captures(l) {
            current_file = Some(c[1].to_owned());
            continue;
        }
        let Some(c) = head.captures(l) else { continue };
        let name = c[1].trim().to_owned();
        if name.starts_with("Test suite failed") {
            continue;
        }
        let following = &lines[i + 1..lines.len().min(i + 20)];
        let message = following
            .iter()
            .map(|n| n.trim())
            .find(|n| !n.is_empty())
            .unwrap_or("failed")
            .to_owned();
        let loc = following.iter().find_map(|n| at.captures(n));
        let file = loc
            .as_ref()
            .map(|c| c[1].to_owned())
            .or_else(|| current_file.clone());
        let test = name.rsplit(" › ").next().unwrap_or(&name).to_owned();
        out.push(Failure {
            rerun: file.as_ref().map(|f| format!("npx jest {f} -t \"{test}\"")),
            test: Some(name),
            line: loc.as_ref().and_then(|c| c[2].parse().ok()),
            file,
            message,
        });
    }
    out
}

/// `src/a.ts(12,5): error TS2322: message`, or `src/a.ts:12:5 - error TS2322: message`.
fn tsc(output: &str) -> Vec<Failure> {
    let paren = re(r"^(\S+?)\((\d+),\d+\): error (TS\d+): (.+)$");
    let colon = re(r"^(\S+?):(\d+):\d+ - error (TS\d+): (.+)$");
    output
        .lines()
        .filter_map(|l| paren.captures(l).or_else(|| colon.captures(l)))
        .map(|c| Failure {
            test: None,
            file: Some(c[1].to_owned()),
            line: c[2].parse().ok(),
            message: format!("{}: {}", &c[3], &c[4]),
            rerun: None,
        })
        .collect()
}

/// eslint's stylish format: a file line, then `  12:5  error  message  rule`.
fn eslint(output: &str) -> Vec<Failure> {
    let item = re(r"^\s+(\d+):\d+\s+error\s+(.+?)(?:\s{2,}(\S+))?$");
    let mut out = Vec::new();
    let mut file: Option<String> = None;
    for l in output.lines() {
        if !l.starts_with(' ') && !l.trim().is_empty() && l.contains('/') && !l.contains("problem")
        {
            file = Some(l.trim().to_owned());
        } else if let Some(c) = item.captures(l) {
            let rule = c
                .get(3)
                .map(|m| format!(" ({})", m.as_str()))
                .unwrap_or_default();
            out.push(Failure {
                test: None,
                file: file.clone(),
                line: c[1].parse().ok(),
                message: format!("{}{rule}", c[2].trim()),
                rerun: file.as_ref().map(|f| format!("npx eslint {f}")),
            });
        }
    }
    out
}

/// `path/to/file.ext:12: error: message` and close variants.
fn generic(output: &str) -> Vec<Failure> {
    let line =
        re(r"^([\w./-]+\.[A-Za-z]+):(\d+)(?::\d+)?:?\s*(?:(?i:error|fail(?:ed|ure)?)\b:?)\s*(.+)$");
    output
        .lines()
        .filter_map(|l| line.captures(l.trim()))
        .map(|c| Failure {
            test: None,
            file: Some(c[1].to_owned()),
            line: c[2].parse().ok(),
            message: c[3].trim().to_owned(),
            rerun: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_test_failures_with_location_and_rerun() {
        let out = "running 2 tests\ntest tests::a ... ok\ntest tests::b ... FAILED\n\nfailures:\n\n---- tests::b stdout ----\n\nthread 'tests::b' (123) panicked at crates/yuzu/src/wait.rs:612:9:\nassertion `left == right` failed\n  left: 1\n right: 2\nnote: run with `RUST_BACKTRACE=1`\n\n\nfailures:\n    tests::b\n\ntest result: FAILED. 1 passed; 1 failed\n";
        let f = failures("cargo test -p mochi-yuzu", out);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].test.as_deref(), Some("tests::b"));
        assert_eq!(
            (f[0].file.as_deref(), f[0].line),
            (Some("crates/yuzu/src/wait.rs"), Some(612))
        );
        assert!(f[0].message.starts_with("assertion `left == right` failed"));
        assert_eq!(
            f[0].rerun.as_deref(),
            Some("cargo test -p mochi-yuzu -- tests::b --exact")
        );
    }

    #[test]
    fn rust_compile_and_clippy_errors() {
        let out = "error[E0425]: cannot find value `x` in this scope\n  --> crates/ube/src/plan.rs:12:5\n   |\nerror: this `if` statement can be collapsed\n --> crates/ube/src/run.rs:40:9\nerror: could not compile `mochi-ube`\n";
        let f = failures("cargo clippy --all-targets", out);
        assert_eq!(f.len(), 2, "{f:?}");
        assert_eq!(
            (f[0].file.as_deref(), f[0].line),
            (Some("crates/ube/src/plan.rs"), Some(12))
        );
        assert_eq!(f[1].message, "this `if` statement can be collapsed");
    }

    #[test]
    fn pytest_summary_lines() {
        let out = "===== short test summary info =====\nFAILED tests/test_money.py::test_round - assert 3 == 4\nERROR tests/test_db.py - ImportError: no module\n";
        let f = failures("python -m pytest", out);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].test.as_deref(), Some("test_round"));
        assert_eq!(f[0].message, "assert 3 == 4");
        assert_eq!(
            f[0].rerun.as_deref(),
            Some("python -m pytest 'tests/test_money.py::test_round'")
        );
        assert_eq!(f[1].file.as_deref(), Some("tests/test_db.py"));
    }

    #[test]
    fn go_test_failures() {
        let out = "=== RUN   TestToken\n--- FAIL: TestToken (0.00s)\n    token_test.go:21: got \"a\", want \"b\"\nFAIL\nFAIL\texample.com/svc/internal/auth\t0.01s\n";
        let f = failures("go test ./internal/auth", out);
        assert_eq!(f.len(), 1);
        assert_eq!(
            (f[0].file.as_deref(), f[0].line),
            (Some("token_test.go"), Some(21))
        );
        assert_eq!(f[0].message, "got \"a\", want \"b\"");
        assert_eq!(
            f[0].rerun.as_deref(),
            Some("go test example.com/svc/internal/auth -run '^TestToken$'")
        );
    }

    #[test]
    fn vitest_failures() {
        let out = " FAIL  src/lib/money.test.ts > formatCents > rounds half even 3ms\nAssertionError: expected '1.01' to be '1.02'\n ❯ src/lib/money.test.ts:14:22\n";
        let f = failures("pnpm test", out);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].test.as_deref(), Some("formatCents > rounds half even"));
        assert_eq!(f[0].line, Some(14));
        assert_eq!(
            f[0].rerun.as_deref(),
            Some("npx vitest run src/lib/money.test.ts -t \"rounds half even\"")
        );
    }

    #[test]
    fn jest_failures() {
        let out = "FAIL src/App.test.tsx\n  ● App › renders the title\n\n    expect(received).toBe(expected)\n\n      at Object.<anonymous> (src/App.test.tsx:9:20)\n";
        let f = failures("npm test", out);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].message, "expect(received).toBe(expected)");
        assert_eq!(
            (f[0].file.as_deref(), f[0].line),
            (Some("src/App.test.tsx"), Some(9))
        );
    }

    #[test]
    fn tsc_eslint_and_the_generic_fallback() {
        let f = failures(
            "npm run typecheck",
            "src/a.ts(3,7): error TS2322: Type 'string' is not assignable to type 'number'.\n",
        );
        assert_eq!(
            f[0].message,
            "TS2322: Type 'string' is not assignable to type 'number'."
        );
        let f = failures(
            "npm run lint",
            "/repo/src/a.ts\n  4:1  error  Unexpected var  no-var\n\n✖ 1 problem\n",
        );
        assert_eq!(f[0].message, "Unexpected var (no-var)");
        assert_eq!(f[0].line, Some(4));
        let f = failures("make check", "lib/a.c:10:3: error: expected ';'\n");
        assert_eq!(
            (f[0].file.as_deref(), f[0].message.as_str()),
            (Some("lib/a.c"), "expected ';'")
        );
        assert!(failures("make check", "all good\n").is_empty());
    }
}
