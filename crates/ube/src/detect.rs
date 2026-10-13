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
    /// Rust lints the repo runs as checks, and the file that says so.
    pub rust_lints: RustLints,
}

/// Whether the repo checks Rust formatting and clippy, from its CI,
/// Makefile, justfile, xtask, or Cargo config.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RustLints {
    /// The file that runs `cargo fmt --check`.
    pub fmt: Option<String>,
    /// The file that runs `cargo clippy`.
    pub clippy: Option<String>,
    /// Whether warnings fail the build (`-D warnings`).
    pub deny_warnings: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CargoPkg {
    pub name: String,
    /// Repo-relative, `""` for the root.
    pub dir: String,
    /// Directories of path dependencies, repo-relative.
    pub deps: Vec<String>,
    /// Dependencies inherited from the workspace, by name, until resolved.
    #[serde(skip)]
    pub inherited: Vec<String>,
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
    /// Repo-relative directory it runs in (`""` for the root).
    pub dir: String,
    /// The job it's in, and the build an earlier step of that job runs,
    /// whose output it may read (`npm run build`).
    pub job: String,
    pub after: Option<String>,
}

/// One command a CI step runs, where, and in which job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiStep {
    pub job: String,
    pub dir: String,
    pub command: String,
}

/// Reads the project from `root`'s tracked files.
pub fn detect(root: &Path, tracked: &[String]) -> Project {
    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).unwrap_or_default();
    let mut p = Project::default();
    let set: BTreeSet<&str> = tracked.iter().map(String::as_str).collect();
    let mut ws_paths: Vec<(String, String)> = Vec::new();

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
                ws_paths.extend(workspace_paths(&text, dir));
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
        let script = workflow
            || matches!(
                f.as_str(),
                "Makefile"
                    | "makefile"
                    | "GNUmakefile"
                    | "justfile"
                    | "Justfile"
                    | ".cargo/config.toml"
                    | ".cargo/config"
            )
            || (f.starts_with("xtask/") && f.ends_with(".rs"));
        if script {
            // A workflow's commands, not its setup: `components: clippy`
            // installs clippy without running it.
            let text = if workflow {
                ci_commands(&read(f)).join("\n")
            } else if f.ends_with(".rs") {
                read(f)
            } else {
                // Makefile, justfile, and TOML comments start with `#`.
                read(f)
                    .lines()
                    .map(|l| l.split('#').next().unwrap_or(""))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let (fmt, clippy, deny) = rust_lints(&text);
            let l = &mut p.rust_lints;
            if fmt && l.fmt.is_none() {
                l.fmt = Some(f.clone());
            }
            if clippy && l.clippy.is_none() {
                l.clippy = Some(f.clone());
            }
            l.deny_warnings |= deny;
        }
        if workflow {
            let mut built: Vec<(String, String)> = Vec::new();
            for step in ci_steps(&read(f)) {
                if is_build_command(&step.command) {
                    built.retain(|(j, _)| *j != step.job);
                    built.push((step.job.clone(), step.command.clone()));
                }
                if is_check_command(&step.command) {
                    let c = CiCheck {
                        file: f.clone(),
                        after: built
                            .iter()
                            .find(|(j, _)| *j == step.job)
                            .map(|(_, b)| b.clone()),
                        command: step.command,
                        dir: step.dir,
                        job: step.job,
                    };
                    if !p
                        .ci
                        .iter()
                        .any(|x| x.command == c.command && x.dir == c.dir)
                    {
                        p.ci.push(c);
                    }
                }
            }
        }
    }
    for pkg in &mut p.cargo {
        for name in std::mem::take(&mut pkg.inherited) {
            if let Some((_, dir)) = ws_paths.iter().find(|(n, _)| *n == name)
                && !pkg.deps.contains(dir)
            {
                pkg.deps.push(dir.clone());
            }
        }
    }
    if p.cargo_root.is_none() && p.cargo.iter().any(|c| c.dir.is_empty()) {
        p.cargo_root = Some(String::new());
    }
    p
}

/// Whether a script or config file checks formatting, runs clippy, and
/// denies warnings.
fn rust_lints(text: &str) -> (bool, bool, bool) {
    let fmt = text.contains("fmt") && text.contains("--check");
    let clippy = text.contains("clippy");
    let deny = regex::Regex::new(r#"-D["',\s]*warnings"#).is_ok_and(|re| re.is_match(text));
    (fmt, clippy, deny)
}

fn split(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

/// A Cargo.toml's package name and path dependencies. Dependencies taken
/// from the workspace (`core = { workspace = true }`) are kept by name in
/// `inherited`, for `detect` to resolve against the workspace's table.
fn cargo_package(text: &str, dir: &str) -> Option<CargoPkg> {
    let mut name = None;
    let mut deps = Vec::new();
    let mut inherited = Vec::new();
    for (section, key, value) in toml_entries(text) {
        if section == "package" && key == "name" {
            name = Some(unquote(value).to_owned());
        }
        if let Some((dep, field, value)) = dependency(&section, key, value) {
            match field {
                "path" => deps.push(join(dir, unquote(value))),
                "workspace" if value == "true" => inherited.push(dep.to_owned()),
                _ => {}
            }
        }
    }
    Some(CargoPkg {
        name: name?,
        dir: dir.to_owned(),
        deps,
        inherited,
    })
}

/// The workspace's `[workspace.dependencies]` that are paths: name to
/// repo-relative directory.
fn workspace_paths(text: &str, dir: &str) -> Vec<(String, String)> {
    toml_entries(text)
        .into_iter()
        .filter_map(|(section, key, value)| {
            let (dep, field, value) = dependency(&section, key, value)?;
            (field == "path" && section.starts_with("workspace."))
                .then(|| (dep.to_owned(), join(dir, unquote(value))))
        })
        .collect()
}

/// `(section, key, value)` for each `key = value` line: enough TOML for
/// Cargo manifests. An inline table's fields come out one by one, as if
/// written `key.field = value`.
fn toml_entries(text: &str) -> Vec<(String, &str, &str)> {
    let mut section = String::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(s) = t.strip_prefix('[') {
            section = s
                .trim_start_matches('[')
                .split(']')
                .next()
                .unwrap_or("")
                .trim()
                .to_owned();
            continue;
        }
        let Some((key, value)) = t.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key.starts_with('#') {
            continue;
        }
        out.push((section.clone(), key, value));
    }
    out
}

/// A dependency's name and one of its fields, from a manifest entry in a
/// `dependencies` section: `core = { path = ".." }`, `core.path = ".."`,
/// or `path = ".."` under `[dependencies.core]`.
fn dependency<'a>(
    section: &'a str,
    key: &'a str,
    value: &'a str,
) -> Option<(&'a str, &'a str, &'a str)> {
    let last = section.rsplit('.').next().unwrap_or(section);
    if last.ends_with("dependencies") {
        if let Some((dep, field)) = key.split_once('.') {
            return Some((dep.trim(), field.trim(), value));
        }
        let fields = value.strip_prefix('{')?.trim_end_matches('}');
        // Only the field we look for matters: path or workspace.
        for f in fields.split(',') {
            if let Some((k, v)) = f.split_once('=') {
                let k = k.trim();
                if k == "path" || k == "workspace" {
                    return Some((
                        key,
                        if k == "path" { "path" } else { "workspace" },
                        v.trim(),
                    ));
                }
            }
        }
        return None;
    }
    // [dependencies.core] or [target.'cfg(unix)'.dependencies.core]
    let (parent, dep) = section.rsplit_once('.')?;
    parent
        .ends_with("dependencies")
        .then_some((dep, key, value))
}

fn unquote(v: &str) -> &str {
    v.trim().trim_matches(['"', '\''])
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
    ci_steps(text).into_iter().map(|s| s.command).collect()
}

/// Each command CI runs, one per line, with its job and the directory it
/// runs in: the step's `working-directory`, else the job's
/// `defaults.run.working-directory`, else the root. GitLab's `script:`
/// items run at the root.
pub fn ci_steps(text: &str) -> Vec<CiStep> {
    struct Step {
        commands: Vec<String>,
        dir: Option<String>,
    }
    let mut out = Vec::new();
    let mut job = String::new();
    let mut job_dir = String::new();
    // Indent of the keys under `jobs:`, once seen.
    let mut job_indent: Option<usize> = None;
    let mut in_jobs = false;
    // Indent of a step's `- `, and the step being read.
    let mut dash: Option<usize> = None;
    let mut step: Option<Step> = None;
    // The column of a `run: |` key: deeper lines are its script.
    let mut block: Option<usize> = None;
    let flush = |step: &mut Option<Step>, job: &str, job_dir: &str, out: &mut Vec<CiStep>| {
        if let Some(s) = step.take() {
            let dir = s.dir.unwrap_or_else(|| job_dir.to_owned());
            out.extend(s.commands.into_iter().map(|command| CiStep {
                job: job.to_owned(),
                dir: dir.clone(),
                command,
            }));
        }
    };
    for line in text.lines() {
        let indent = line.len() - line.trim_start().len();
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(col) = block {
            if indent > col {
                let cmd = t.trim_start_matches("- ").to_owned();
                step.get_or_insert(Step {
                    commands: Vec::new(),
                    dir: None,
                })
                .commands
                .push(cmd);
                continue;
            }
            block = None;
        }
        if indent == 0 {
            flush(&mut step, &job, &job_dir, &mut out);
            in_jobs = t == "jobs:";
            dash = None;
            // GitLab: top-level keys are jobs.
            if !in_jobs && t.ends_with(':') {
                job = t.trim_end_matches(':').to_owned();
                job_dir.clear();
            }
            continue;
        }
        if in_jobs
            && job_indent.is_none_or(|j| j == indent)
            && t.ends_with(':')
            && !t.starts_with("- ")
        {
            flush(&mut step, &job, &job_dir, &mut out);
            job_indent = Some(indent);
            job = t.trim_end_matches(':').to_owned();
            job_dir.clear();
            dash = None;
            continue;
        }
        let (body, col) = match t.strip_prefix("- ") {
            Some(rest) => {
                if dash.is_none_or(|d| d == indent) {
                    flush(&mut step, &job, &job_dir, &mut out);
                    dash = Some(indent);
                    step = Some(Step {
                        commands: Vec::new(),
                        dir: None,
                    });
                }
                (rest, indent + 2)
            }
            None => (t, indent),
        };
        if dash.is_some_and(|d| indent < d) {
            // Back out of the steps list, into the job's other keys.
            flush(&mut step, &job, &job_dir, &mut out);
            dash = None;
        }
        if let Some(dir) = body.strip_prefix("working-directory:") {
            let dir = dir
                .trim()
                .trim_matches(['"', '\''])
                .trim_start_matches("./");
            let dir = match dir.trim_end_matches('/') {
                "." => String::new(),
                d => d.to_owned(),
            };
            match step.as_mut() {
                Some(s) if dash.is_some() => s.dir = Some(dir),
                _ => job_dir = dir,
            }
            continue;
        }
        let run = body.strip_prefix("run:");
        if let Some(cmd) = run.or_else(|| body.strip_prefix("script:")) {
            let cmd = cmd.trim();
            if cmd.is_empty() && run.is_some() {
                // `defaults: run:` is a mapping, not a script.
                continue;
            }
            if cmd.is_empty() || cmd.starts_with('|') || cmd.starts_with('>') {
                block = Some(col);
            } else {
                step.get_or_insert(Step {
                    commands: Vec::new(),
                    dir: None,
                })
                .commands
                .push(cmd.trim_matches(['"', '\'']).to_owned());
            }
        }
    }
    flush(&mut step, &job, &job_dir, &mut out);
    out
}

/// Whether a command builds output a later check may read.
fn is_build_command(cmd: &str) -> bool {
    let words: Vec<&str> = cmd.split_whitespace().collect();
    matches!(
        words.as_slice(),
        ["npm" | "pnpm" | "yarn" | "bun", "run", "build", ..]
            | ["npm" | "pnpm" | "yarn" | "bun", "build", ..]
    )
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
    // `format:check` checks; `format` alone rewrites files.
    let script = |s: &str| {
        SCRIPTS.iter().any(|x| s.starts_with(x))
            || s.starts_with("coverage")
            || (s.starts_with("format") && s.contains("check"))
    };
    match first {
        "cargo" => matches!(
            second,
            "test" | "clippy" | "fmt" | "check" | "xtask" | "nextest" | "llvm-cov" | "tarpaulin"
        ),
        "npm" | "pnpm" | "yarn" | "bun" => {
            SCRIPTS.contains(&second) || (second == "run" && script(third))
        }
        // `npx playwright install` sets up browsers; it isn't a check.
        "npx" => {
            matches!(second, "vitest" | "jest" | "tsc" | "eslint" | "playwright")
                && !third.starts_with("install")
        }
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
    fn finds_the_rust_lints_a_repo_runs() {
        let xtask =
            r#"&["fmt", "--all", "--check"], &["clippy", "--workspace", "--", "-D", "warnings"]"#;
        assert_eq!(rust_lints(xtask), (true, true, true));
        let ci = "- run: cargo clippy --all-targets -- -Dwarnings\n";
        assert_eq!(rust_lints(ci), (false, true, true));
        assert_eq!(
            rust_lints("- run: cargo fmt\n- run: cargo clippy\n"),
            (false, true, false)
        );
    }

    #[test]
    fn reads_workspace_inherited_path_deps() {
        let root = "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.dependencies]\ncore = { path = \"crates/core\" }\nserde = \"1\"\n";
        let app = "[package]\nname = \"app\"\n\n[dependencies]\ncore = { workspace = true }\nserde.workspace = true\npathdiff = \"0.2\"\n\n[dev-dependencies.core]\nworkspace = true\n";
        let tracked = ["Cargo.toml", "crates/app/Cargo.toml"].map(String::from);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), root).unwrap();
        std::fs::create_dir_all(dir.path().join("crates/app")).unwrap();
        std::fs::write(dir.path().join("crates/app/Cargo.toml"), app).unwrap();
        let p = detect(dir.path(), &tracked);
        assert_eq!(p.cargo.len(), 1);
        assert_eq!(p.cargo[0].deps, ["crates/core"]);
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
    fn reads_ci_working_directories_and_jobs() {
        let gh = "jobs:\n  backend:\n    runs-on: x\n    steps:\n      - uses: actions/checkout@v4\n      - run: cargo fmt --all -- --check\n        working-directory: backend\n      - name: Test\n        working-directory: ./backend/\n        run: cargo test\n      - run: |\n          echo one\n          echo two\n        working-directory: scripts\n  frontend:\n    defaults:\n      run:\n        working-directory: frontend\n    steps:\n      - run: npm ci\n      - run: npm run build\n      - run: npm run check:bundle\n      - run: echo root\n        working-directory: .\n";
        let got: Vec<(String, String, String)> = ci_steps(gh)
            .into_iter()
            .map(|s| (s.job, s.dir, s.command))
            .collect();
        let want = [
            ("backend", "backend", "cargo fmt --all -- --check"),
            ("backend", "backend", "cargo test"),
            ("backend", "scripts", "echo one"),
            ("backend", "scripts", "echo two"),
            ("frontend", "frontend", "npm ci"),
            ("frontend", "frontend", "npm run build"),
            ("frontend", "frontend", "npm run check:bundle"),
            ("frontend", "", "echo root"),
        ]
        .map(|(a, b, c)| (a.to_owned(), b.to_owned(), c.to_owned()));
        assert_eq!(got, want);
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
            "cargo llvm-cov --workspace --fail-under-lines 90",
            "npm run coverage",
            "npm run format:check",
        ] {
            assert!(is_check_command(c), "{c}");
        }
        for c in [
            "npm ci",
            "cargo build --release",
            "echo hi",
            "pip install -r req.txt",
            "make install",
            "npx playwright install --with-deps chromium",
            "npm run format",
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
