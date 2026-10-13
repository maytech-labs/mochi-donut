//! What checks a repo declares: Cargo packages and their path dependencies,
//! npm packages and their scripts, Go modules, Python test setups, Makefile
//! targets, and the check commands its CI runs. Read from tracked files only
//! (docs/ube/design.md, "ube plan").

use std::collections::BTreeSet;
use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Project {
    pub cargo: Vec<CargoPkg>,
    /// The directory of the Cargo workspace root (`""` for the repo root).
    pub cargo_root: Option<String>,
    pub npm: Vec<NpmPkg>,
    pub go: Vec<String>,
    pub python: Vec<String>,
    pub make_targets: Vec<String>,
    pub ci: Vec<CiCheck>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CargoPkg {
    pub name: String,
    /// Repo-relative, `""` for the root.
    pub dir: String,
    /// Directories of path dependencies, repo-relative.
    pub deps: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NpmPkg {
    pub dir: String,
    /// `npm`, `pnpm`, `yarn`, or `bun`, from the lockfile beside it or above.
    pub manager: &'static str,
    pub scripts: Vec<String>,
}

/// A check command CI runs, from a workflow file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CiCheck {
    pub file: String,
    pub command: String,
}

/// Reads the project from `root`'s tracked files.
pub fn detect(root: &Path, tracked: &[String]) -> Project {
    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).unwrap_or_default();
    let mut p = Project::default();
    let set: BTreeSet<&str> = tracked.iter().map(String::as_str).collect();

    for f in tracked {
        let (dir, name) = split(f);
        match name {
            "Cargo.toml" => {
                let text = read(f);
                if text.contains("[workspace]")
                    && p.cargo_root.as_deref().is_none_or(|r| dir.len() < r.len())
                {
                    p.cargo_root = Some(dir.to_owned());
                }
                if let Some(pkg) = cargo_package(&text, dir) {
                    p.cargo.push(pkg);
                }
            }
            "package.json" => {
                let scripts = serde_json::from_str::<serde_json::Value>(&read(f))
                    .ok()
                    .and_then(|v| {
                        v.get("scripts")
                            .and_then(|s| s.as_object())
                            .map(|o| o.keys().cloned().collect())
                    })
                    .unwrap_or_default();
                p.npm.push(NpmPkg {
                    dir: dir.to_owned(),
                    manager: npm_manager(dir, &set),
                    scripts,
                });
            }
            "go.mod" => p.go.push(dir.to_owned()),
            "pyproject.toml" | "pytest.ini" | "setup.cfg" | "tox.ini" | "setup.py" => {
                if !p.python.contains(&dir.to_owned()) {
                    p.python.push(dir.to_owned());
                }
            }
            "Makefile" | "makefile" | "GNUmakefile" if dir.is_empty() => {
                p.make_targets = make_targets(&read(f));
            }
            _ => {}
        }
        let workflow = (f.starts_with(".github/workflows/")
            && (f.ends_with(".yml") || f.ends_with(".yaml")))
            || f == ".gitlab-ci.yml";
        if workflow {
            for command in ci_commands(&read(f)) {
                if is_check_command(&command) {
                    let c = CiCheck {
                        file: f.clone(),
                        command,
                    };
                    if !p.ci.contains(&c) {
                        p.ci.push(c);
                    }
                }
            }
        }
    }
    if p.cargo_root.is_none() && p.cargo.iter().any(|c| c.dir.is_empty()) {
        p.cargo_root = Some(String::new());
    }
    p
}

fn split(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

/// A Cargo.toml's package name and path dependencies.
fn cargo_package(text: &str, dir: &str) -> Option<CargoPkg> {
    let mut section = "";
    let mut name = None;
    let mut deps = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            section = t;
            continue;
        }
        if section == "[package]"
            && let Some(v) = t
                .strip_prefix("name")
                .map(str::trim_start)
                .and_then(|r| r.strip_prefix('='))
        {
            name = Some(v.trim().trim_matches('"').to_owned());
        }
        if section.contains("dependencies")
            && let Some(i) = t.find("path")
            && let Some(q) = t[i..].find('"')
        {
            let rest = &t[i + q + 1..];
            if let Some(end) = rest.find('"') {
                deps.push(join(dir, &rest[..end]));
            }
        }
    }
    Some(CargoPkg {
        name: name?,
        dir: dir.to_owned(),
        deps,
    })
}

/// `dir` joined with a relative path, with `..` resolved.
pub fn join(dir: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn npm_manager(dir: &str, files: &BTreeSet<&str>) -> &'static str {
    let mut d = dir.to_owned();
    loop {
        let at = |name: &str| {
            if d.is_empty() {
                name.to_owned()
            } else {
                format!("{d}/{name}")
            }
        };
        if files.contains(at("pnpm-lock.yaml").as_str()) {
            return "pnpm";
        }
        if files.contains(at("yarn.lock").as_str()) {
            return "yarn";
        }
        if files.contains(at("bun.lockb").as_str()) || files.contains(at("bun.lock").as_str()) {
            return "bun";
        }
        if files.contains(at("package-lock.json").as_str()) || d.is_empty() {
            return "npm";
        }
        d = d
            .rsplit_once('/')
            .map_or(String::new(), |(p, _)| p.to_owned());
    }
}

fn make_targets(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !l.starts_with(['\t', ' ', '#', '.']))
        .filter_map(|l| {
            let (names, rest) = l.split_once(':')?;
            (!rest.starts_with('=') && !names.contains(['=', '$', '%'])).then_some(names)
        })
        .flat_map(|n| n.split_whitespace().map(str::to_owned))
        .collect()
}

/// `run:` steps from a GitHub workflow, and `script:` items from GitLab CI,
/// one command per line.
pub fn ci_commands(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut block: Option<usize> = None;
    for line in text.lines() {
        let indent = line.len() - line.trim_start().len();
        let t = line.trim();
        if let Some(min) = block {
            if t.is_empty() {
                continue;
            }
            if indent > min {
                out.push(t.trim_start_matches("- ").to_owned());
                continue;
            }
            block = None;
        }
        let body = t.strip_prefix("- ").unwrap_or(t);
        if let Some(cmd) = body
            .strip_prefix("run:")
            .or_else(|| body.strip_prefix("script:"))
        {
            let cmd = cmd.trim();
            if cmd.is_empty() || cmd.starts_with('|') || cmd.starts_with('>') {
                block = Some(indent);
            } else {
                out.push(cmd.trim_matches(['"', '\'']).to_owned());
            }
        }
    }
    out
}

/// Whether a command runs tests, linters, or type checks, as opposed to
/// setup (installs, checkouts, caches).
pub fn is_check_command(cmd: &str) -> bool {
    let words: Vec<&str> = cmd.split_whitespace().collect();
    let first = words
        .first()
        .map(|w| w.rsplit('/').next().unwrap_or(w))
        .unwrap_or("");
    let second = words.get(1).copied().unwrap_or("");
    let third = words.get(2).copied().unwrap_or("");
    const SCRIPTS: &[&str] = &[
        "test",
        "lint",
        "check",
        "typecheck",
        "type-check",
        "tsc",
        "e2e",
        "vet",
    ];
    match first {
        "cargo" => matches!(
            second,
            "test" | "clippy" | "fmt" | "check" | "xtask" | "nextest"
        ),
        "npm" | "pnpm" | "yarn" | "bun" => {
            SCRIPTS.contains(&second)
                || (second == "run" && SCRIPTS.iter().any(|s| third.starts_with(s)))
        }
        "npx" => matches!(second, "vitest" | "jest" | "tsc" | "eslint" | "playwright"),
        "go" => matches!(second, "test" | "vet"),
        "pytest" | "ruff" | "mypy" | "tsc" | "eslint" | "vitest" | "jest" | "golangci-lint" => true,
        "python" | "python3" => {
            second == "-m" && matches!(third, "pytest" | "mypy" | "ruff" | "unittest")
        }
        "make" | "just" => SCRIPTS.iter().any(|s| second.starts_with(s)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_cargo_packages_and_path_deps() {
        let text = "[package]\nname = \"mochi-yuzu\"\n\n[dependencies]\nmochi-common = { version = \"0.0.0\", path = \"../common\" }\nserde = \"1\"\n\n[dev-dependencies]\nhelper = { path = \"../../tools/helper\" }\n";
        let p = cargo_package(text, "crates/yuzu").unwrap();
        assert_eq!(p.name, "mochi-yuzu");
        assert_eq!(p.deps, ["crates/common", "tools/helper"]);
        assert!(cargo_package("[workspace]\nmembers = []\n", "").is_none());
    }

    #[test]
    fn reads_ci_commands_from_both_forms() {
        let gh = "jobs:\n  check:\n    steps:\n      - uses: actions/checkout@v4\n      - run: cargo xtask check\n      - name: tests\n        run: |\n          npm ci\n          npm test\n      - run: echo done\n";
        assert_eq!(
            ci_commands(gh),
            ["cargo xtask check", "npm ci", "npm test", "echo done"]
        );
        let gl = "test:\n  script:\n    - go vet ./...\n    - go test ./...\n";
        assert_eq!(ci_commands(gl), ["go vet ./...", "go test ./..."]);
    }

    #[test]
    fn tells_checks_from_setup() {
        for c in [
            "cargo xtask check",
            "npm test",
            "pnpm run lint:ci",
            "go test ./...",
            "python -m pytest -q",
            "make check",
            "npx vitest run",
        ] {
            assert!(is_check_command(c), "{c}");
        }
        for c in [
            "npm ci",
            "cargo build --release",
            "echo hi",
            "pip install -r req.txt",
            "make install",
        ] {
            assert!(!is_check_command(c), "{c}");
        }
    }

    #[test]
    fn picks_the_package_manager_from_lockfiles() {
        let files: BTreeSet<&str> = [
            "web/package.json",
            "pnpm-lock.yaml",
            "api/package.json",
            "api/yarn.lock",
        ]
        .into();
        assert_eq!(npm_manager("web", &files), "pnpm");
        assert_eq!(npm_manager("api", &files), "yarn");
        assert_eq!(npm_manager("", &BTreeSet::new()), "npm");
    }
}
