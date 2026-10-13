//! `ube plan`: which of the repo's declared checks apply to a change, why,
//! and what was left out (docs/ube/design.md, "ube plan").

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::detect::{CargoPkg, Project};

/// One check to run: a shell command in a directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    /// The command, run with `sh -c`.
    pub command: String,
    /// Repo-relative directory to run it in (`""` for the root).
    pub dir: String,
    /// Why it's in the plan.
    pub reason: String,
    /// Whether it runs tests, as opposed to linting or type checking.
    pub tests: bool,
}

impl Check {
    /// The key results are stored under.
    pub fn id(&self) -> String {
        if self.dir.is_empty() {
            self.command.clone()
        } else {
            format!("({}) {}", self.dir, self.command)
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LeftOut {
    pub command: String,
    pub why: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub changed: Vec<String>,
    pub checks: Vec<Check>,
    /// Checks the repo declares that the plan doesn't run, and why.
    pub left_out: Vec<LeftOut>,
    /// Changed files no check was found for.
    pub unmatched: Vec<String>,
    /// True when the plan is the full suite: `--all`, or no selection could
    /// be made for some changed file.
    pub full: bool,
}

/// A plan for `changed` files. `matcha` says whether docs can be checked.
pub fn plan(project: &Project, changed: &[String], all: bool, matcha: bool) -> Plan {
    let mut checks: Vec<Check> = Vec::new();
    let mut unmatched = Vec::new();
    let mut docs: Vec<String> = Vec::new();
    let add = |checks: &mut Vec<Check>, c: Check| {
        if !checks.iter().any(|x| same_check(x, &c)) {
            checks.push(c);
        }
    };

    // Cargo packages holding a changed file, for the repo's Rust lints;
    // `None` inside means a workspace-wide change.
    let mut linted: Vec<Option<&str>> = Vec::new();
    let cargo_root = project.cargo_root.clone().unwrap_or_default();
    if !all {
        for file in changed {
            let mut matched = false;
            let (dir, name) = file.rsplit_once('/').unwrap_or(("", file.as_str()));

            // Cargo: the package that holds the file, and every package that
            // depends on it through a path dependency.
            if project.cargo_root.is_some() {
                if matches!(name, "Cargo.lock" | "rust-toolchain.toml")
                    || (name == "Cargo.toml" && dir == cargo_root)
                    || workspace_file(file, &cargo_root, &project.cargo)
                {
                    add(
                        &mut checks,
                        Check {
                            command: "cargo test --workspace".into(),
                            dir: cargo_root.clone(),
                            reason: format!("{file} affects every package"),
                            tests: true,
                        },
                    );
                    linted.push(None);
                    matched = true;
                } else if let Some(pkg) = owner(&project.cargo, file) {
                    if name.ends_with(".rs") && !linted.contains(&Some(pkg.name.as_str())) {
                        linted.push(Some(pkg.name.as_str()));
                    }
                    for p in with_dependents(&project.cargo, pkg) {
                        let why = if p.name == pkg.name {
                            format!("{file} is in {}", p.name)
                        } else {
                            format!("{} depends on {} ({file} changed)", p.name, pkg.name)
                        };
                        add(
                            &mut checks,
                            Check {
                                command: format!("cargo test -p {}", p.name),
                                dir: cargo_root.clone(),
                                reason: why,
                                tests: true,
                            },
                        );
                    }
                    matched = true;
                }
            }

            // npm: the nearest package.json's test and lint scripts, and the
            // checks CI runs in it.
            if let Some(pkg) = nearest(project.npm.iter().map(|p| p.dir.as_str()), file)
                .and_then(|d| project.npm.iter().find(|p| p.dir == d))
            {
                for c in npm_ci_checks(project, &pkg.dir) {
                    add(
                        &mut checks,
                        Check {
                            command: c.command.clone(),
                            dir: pkg.dir.clone(),
                            reason: format!(
                                "{file} is in the package at {}; CI runs it there ({})",
                                display_dir(&pkg.dir),
                                c.file
                            ),
                            tests: false,
                        },
                    );
                    matched = true;
                }
                for script in ["test", "lint", "typecheck", "check"] {
                    if pkg.scripts.iter().any(|s| s == script) {
                        let command = match (pkg.manager, script) {
                            ("bun", s) => format!("bun run {s}"),
                            (m, "test") => format!("{m} test"),
                            (m, s) => format!("{m} run {s}"),
                        };
                        add(
                            &mut checks,
                            Check {
                                command,
                                dir: pkg.dir.clone(),
                                reason: format!(
                                    "{file} is in the package at {}",
                                    display_dir(&pkg.dir)
                                ),
                                tests: script == "test",
                            },
                        );
                        matched = true;
                    }
                }
            }

            // Go: the package directory, or every package for go.mod changes.
            if let Some(module) = nearest(project.go.iter().map(String::as_str), file) {
                let rel = strip_dir(dir, module);
                if name == "go.mod" || name == "go.sum" {
                    add(
                        &mut checks,
                        Check {
                            command: "go test ./...".into(),
                            dir: module.to_owned(),
                            reason: format!("{file} affects every package"),
                            tests: true,
                        },
                    );
                    matched = true;
                } else if name.ends_with(".go") {
                    let target = if rel.is_empty() {
                        ".".to_owned()
                    } else {
                        format!("./{rel}")
                    };
                    add(
                        &mut checks,
                        Check {
                            command: format!("go test {target}"),
                            dir: module.to_owned(),
                            reason: format!("{file} is in Go package {target}"),
                            tests: true,
                        },
                    );
                    matched = true;
                }
            }

            // Python: pytest for the project that holds the file.
            if name.ends_with(".py")
                && let Some(proj) = nearest(project.python.iter().map(String::as_str), file)
            {
                add(
                    &mut checks,
                    Check {
                        command: "python -m pytest".into(),
                        dir: proj.to_owned(),
                        reason: format!("{file} is in the Python project at {}", display_dir(proj)),
                        tests: true,
                    },
                );
                matched = true;
            }

            // Docs: one matcha drift run for all of them, added below.
            if (name.ends_with(".md") || name.ends_with(".mdx")) && matcha {
                docs.push(file.clone());
                matched = true;
            }

            if !matched {
                unmatched.push(file.clone());
            }
        }
    }

    if !docs.is_empty() {
        let ins: Vec<String> = docs.iter().map(|d| format!("--in {d}")).collect();
        let reason = match docs.as_slice() {
            [one] => format!("{one} is a doc"),
            many => format!("{} docs changed", many.len()),
        };
        add(
            &mut checks,
            Check {
                command: format!("matcha drift {}", ins.join(" ")),
                dir: String::new(),
                reason,
                tests: false,
            },
        );
    }

    for mut c in rust_lints(project, &linted, &cargo_root) {
        // The repo's own spelling, when CI runs the same check.
        if let Some(ci) = project.ci.iter().find(|ci| {
            same_check(
                &c,
                &Check {
                    command: ci.command.clone(),
                    dir: ci.dir.clone(),
                    ..c.clone()
                },
            )
        }) {
            c.command.clone_from(&ci.command);
        }
        add(&mut checks, c);
    }

    // A workspace-wide test covers each package's.
    if checks.iter().any(|c| c.command == "cargo test --workspace") {
        checks.retain(|c| !c.command.starts_with("cargo test -p "));
    }

    // When something has no check of its own, or with --all, run what the
    // repo calls its full check: CI's check commands, else make check/test.
    let full_suite = full_suite(project);
    let full = all || !unmatched.is_empty();
    let mut left_out = Vec::new();
    if full {
        let why = if all {
            "--all".to_owned()
        } else {
            format!("no check is linked to {}", list(&unmatched))
        };
        for c in &full_suite {
            add(
                &mut checks,
                Check {
                    reason: format!("the full suite ({why})"),
                    ..c.clone()
                },
            );
        }
    } else {
        for c in &full_suite {
            if !checks.iter().any(|x| same_check(x, c)) {
                left_out.push(LeftOut {
                    command: c.command.clone(),
                    why: format!("{}; ube plan --all includes it", c.reason),
                });
            }
        }
    }
    Plan {
        changed: changed.to_vec(),
        checks,
        left_out,
        unmatched,
        full,
    }
}

/// `cargo fmt --check` and `cargo clippy` for the changed packages, when
/// the repo runs them itself, with `-D warnings` when it denies warnings.
/// `None` among `packages` means the whole workspace.
fn rust_lints(project: &Project, packages: &[Option<&str>], dir: &str) -> Vec<Check> {
    if packages.is_empty() {
        return Vec::new();
    }
    let l = &project.rust_lints;
    let (scope, which) = if packages.contains(&None) {
        ("--workspace".to_owned(), "every package".to_owned())
    } else {
        let names: Vec<&str> = packages.iter().flatten().copied().collect();
        (
            names
                .iter()
                .map(|n| format!("-p {n}"))
                .collect::<Vec<_>>()
                .join(" "),
            names.join(", "),
        )
    };
    let mut out = Vec::new();
    if let Some(file) = &l.fmt {
        let scope = if scope == "--workspace" {
            "--all"
        } else {
            scope.as_str()
        };
        out.push(Check {
            command: format!("cargo fmt --check {scope}"),
            dir: dir.to_owned(),
            reason: format!("Rust in {which} changed; {file} checks formatting"),
            tests: false,
        });
    }
    if let Some(file) = &l.clippy {
        let deny = if l.deny_warnings {
            " -- -D warnings"
        } else {
            ""
        };
        out.push(Check {
            command: format!("cargo clippy {scope} --all-targets{deny}"),
            dir: dir.to_owned(),
            reason: format!("Rust in {which} changed; {file} runs clippy"),
            tests: false,
        });
    }
    out
}

/// What the repo runs as its whole check: its CI commands, else `make
/// check`, else `make test`.
fn full_suite(project: &Project) -> Vec<Check> {
    if !project.ci.is_empty() {
        let mut out: Vec<Check> = Vec::new();
        for c in &project.ci {
            // A check that reads a build's output runs after that build.
            if let Some(build) = &c.after {
                let b = Check {
                    command: build.clone(),
                    dir: c.dir.clone(),
                    reason: format!("CI builds before {} ({})", c.command, c.file),
                    tests: false,
                };
                if !out.iter().any(|x| same_check(x, &b)) {
                    out.push(b);
                }
            }
            out.push(Check {
                command: c.command.clone(),
                dir: c.dir.clone(),
                reason: format!("CI runs it ({})", c.file),
                tests: ["test", "check", "cov"]
                    .iter()
                    .any(|w| c.command.contains(w)),
            });
        }
        return out;
    }
    ["check", "test"]
        .iter()
        .find(|t| project.make_targets.iter().any(|m| m == *t))
        .map(|t| Check {
            command: format!("make {t}"),
            dir: String::new(),
            reason: format!("the Makefile's {t} target"),
            tests: true,
        })
        .into_iter()
        .collect()
}

/// Check scripts CI runs in the npm package at `dir`, beyond the usual
/// test, lint, typecheck, and check: `format:check`, `check:api`. Not
/// coverage (the same tests as `test`), and not ones that run after a build
/// in their job, since they may read its output.
fn npm_ci_checks<'a>(project: &'a Project, dir: &str) -> Vec<&'a crate::detect::CiCheck> {
    project
        .ci
        .iter()
        .filter(|c| c.dir == dir && c.after.is_none())
        .filter(|c| {
            let w: Vec<&str> = c.command.split_whitespace().collect();
            matches!(w.as_slice(), ["npm" | "pnpm" | "yarn" | "bun", "run", s, ..] if !s.starts_with("coverage"))
        })
        .collect()
}

/// Whether two checks are the same command in the same place, however its
/// words are ordered: `cargo fmt --all -- --check` is `cargo fmt --check --all`.
fn same_check(a: &Check, b: &Check) -> bool {
    fn words(c: &str) -> Vec<&str> {
        let mut w: Vec<&str> = c.split_whitespace().filter(|w| *w != "--").collect();
        w.sort_unstable();
        w
    }
    a.dir == b.dir && (a.command == b.command || words(&a.command) == words(&b.command))
}

/// Whether `file` sits in the Cargo workspace outside every package and
/// affects how all of them build or lint: sqlx's offline query data,
/// clippy and rustfmt config, Cargo config.
fn workspace_file(file: &str, root: &str, pkgs: &[CargoPkg]) -> bool {
    if !under(file, root) || owner(pkgs, file).is_some_and(|p| p.dir != root) {
        return false;
    }
    let rel = strip_dir(file, root);
    // Under a Cargo workspace, migrations are sqlx's, diesel's, or
    // refinery's, embedded in the build.
    rel.starts_with(".sqlx/")
        || rel.starts_with(".cargo/")
        || rel.starts_with("migrations/")
        || matches!(
            rel,
            "clippy.toml" | ".clippy.toml" | "rustfmt.toml" | ".rustfmt.toml"
        )
}

/// The Cargo package whose directory holds `file`: the deepest one.
fn owner<'a>(pkgs: &'a [CargoPkg], file: &str) -> Option<&'a CargoPkg> {
    pkgs.iter()
        .filter(|p| under(file, &p.dir))
        .max_by_key(|p| p.dir.len())
}

/// `pkg` and every package that depends on it, directly or not.
fn with_dependents<'a>(pkgs: &'a [CargoPkg], pkg: &'a CargoPkg) -> Vec<&'a CargoPkg> {
    let mut out = vec![pkg];
    let mut seen: BTreeSet<&str> = [pkg.dir.as_str()].into();
    let mut i = 0;
    while i < out.len() {
        let dir = out[i].dir.clone();
        for p in pkgs {
            if p.deps.contains(&dir) && seen.insert(p.dir.as_str()) {
                out.push(p);
            }
        }
        i += 1;
    }
    out
}

/// The deepest directory in `dirs` that holds `file`.
fn nearest<'a>(dirs: impl Iterator<Item = &'a str>, file: &str) -> Option<&'a str> {
    dirs.filter(|d| under(file, d)).max_by_key(|d| d.len())
}

fn under(file: &str, dir: &str) -> bool {
    dir.is_empty() || file.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}

fn strip_dir<'a>(dir: &'a str, module: &str) -> &'a str {
    if module.is_empty() {
        dir
    } else {
        dir.strip_prefix(module)
            .map_or(dir, |r| r.trim_start_matches('/'))
    }
}

fn display_dir(dir: &str) -> &str {
    if dir.is_empty() { "the root" } else { dir }
}

fn list(files: &[String]) -> String {
    match files {
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, rest @ ..] => format!("{a} and {} more", rest.len()),
        [] => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::{CiCheck, NpmPkg, RustLints};

    fn project() -> Project {
        Project {
            cargo: vec![
                CargoPkg {
                    name: "mochi-common".into(),
                    dir: "crates/common".into(),
                    deps: vec![],
                    inherited: vec![],
                },
                CargoPkg {
                    name: "mochi-index".into(),
                    dir: "crates/index".into(),
                    deps: vec![],
                    inherited: vec![],
                },
                CargoPkg {
                    name: "mochi-matcha".into(),
                    dir: "crates/matcha".into(),
                    deps: vec!["crates/common".into(), "crates/index".into()],
                    inherited: vec![],
                },
                CargoPkg {
                    name: "mochi-yuzu".into(),
                    dir: "crates/yuzu".into(),
                    deps: vec!["crates/common".into()],
                    inherited: vec![],
                },
            ],
            cargo_root: Some(String::new()),
            npm: vec![NpmPkg {
                dir: "web".into(),
                manager: "pnpm",
                scripts: vec!["test".into(), "lint".into(), "dev".into()],
            }],
            go: vec![],
            python: vec![],
            make_targets: vec!["check".into()],
            ci: vec![CiCheck {
                file: ".github/workflows/ci.yml".into(),
                command: "cargo xtask check".into(),
                dir: String::new(),
                job: "check".into(),
                after: None,
            }],
            rust_lints: RustLints::default(),
        }
    }

    fn commands(p: &Plan) -> Vec<&str> {
        p.checks.iter().map(|c| c.command.as_str()).collect()
    }

    #[test]
    fn a_crate_change_tests_it_and_its_dependents() {
        let p = plan(
            &project(),
            &["crates/common/src/lib.rs".into()],
            false,
            false,
        );
        assert_eq!(
            commands(&p),
            [
                "cargo test -p mochi-common",
                "cargo test -p mochi-matcha",
                "cargo test -p mochi-yuzu"
            ]
        );
        assert!(p.checks[1].reason.contains("depends on mochi-common"));
        assert!(!p.full);
        assert_eq!(p.left_out[0].command, "cargo xtask check");
    }

    #[test]
    fn rust_changes_get_the_lints_the_repo_runs() {
        let mut pr = project();
        // Without declared lints, only tests.
        let p = plan(&pr, &["crates/yuzu/src/a.rs".into()], false, false);
        assert_eq!(commands(&p), ["cargo test -p mochi-yuzu"]);
        pr.rust_lints = RustLints {
            fmt: Some("xtask/src/main.rs".into()),
            clippy: Some("xtask/src/main.rs".into()),
            deny_warnings: true,
        };
        let p = plan(&pr, &["crates/common/src/lib.rs".into()], false, false);
        assert_eq!(
            commands(&p),
            [
                "cargo test -p mochi-common",
                "cargo test -p mochi-matcha",
                "cargo test -p mochi-yuzu",
                "cargo fmt --check -p mochi-common",
                "cargo clippy -p mochi-common --all-targets -- -D warnings"
            ]
        );
        assert!(
            p.checks[3]
                .reason
                .contains("xtask/src/main.rs checks formatting")
        );
        assert!(!p.checks[3].tests && !p.checks[4].tests);
        // A Cargo.toml in a package isn't Rust code: tests only.
        let p = plan(&pr, &["crates/yuzu/Cargo.toml".into()], false, false);
        assert_eq!(commands(&p), ["cargo test -p mochi-yuzu"]);
        let p = plan(&pr, &["Cargo.lock".into()], false, false);
        assert_eq!(
            commands(&p),
            [
                "cargo test --workspace",
                "cargo fmt --check --all",
                "cargo clippy --workspace --all-targets -- -D warnings"
            ]
        );
    }

    #[test]
    fn npm_packages_get_the_checks_ci_runs_in_them() {
        let mut pr = project();
        let ci = |command: &str, dir: &str, after_build: bool| CiCheck {
            file: ".github/workflows/ci.yml".into(),
            command: command.into(),
            dir: dir.into(),
            job: "frontend".into(),
            after: after_build.then(|| "npm run build".to_owned()),
        };
        pr.ci = vec![
            ci("npm run lint", "web", false),
            ci("npm run format:check", "web", false),
            ci("npm run coverage", "web", false),
            ci("npm run check:bundle", "web", true),
            ci("npm run check:api", "other", false),
        ];
        let p = plan(&pr, &["web/src/App.tsx".into()], false, false);
        assert_eq!(
            commands(&p),
            [
                "npm run lint",
                "npm run format:check",
                "pnpm test",
                "pnpm run lint"
            ]
        );
        assert!(p.checks[1].reason.contains("CI runs it there"));
        assert_eq!(p.checks[1].dir, "web");
        // The full suite runs where CI does, building before what reads
        // the build.
        let p = plan(&pr, &[], true, false);
        assert!(p.checks.iter().all(|c| c.dir == "web" || c.dir == "other"));
        let at = |cmd: &str| commands(&p).iter().position(|c| *c == cmd).unwrap();
        assert!(at("npm run build") < at("npm run check:bundle"));
    }

    #[test]
    fn rust_workspace_files_outside_packages_test_the_workspace() {
        let mut pr = project();
        pr.cargo_root = Some("backend".into());
        for c in &mut pr.cargo {
            c.dir = format!("backend/{}", c.dir);
        }
        for f in [
            "backend/.sqlx/query-1.json",
            "backend/clippy.toml",
            "backend/migrations/2026_x.sql",
        ] {
            let p = plan(&pr, &[f.to_owned()], false, false);
            assert_eq!(commands(&p), ["cargo test --workspace"], "{f}");
            assert_eq!(p.checks[0].dir, "backend");
        }
        // Inside a package, it's that package's.
        let p = plan(
            &pr,
            &["backend/crates/yuzu/.sqlx/q.json".into()],
            false,
            false,
        );
        assert_eq!(commands(&p), ["cargo test -p mochi-yuzu"]);
    }

    #[test]
    fn the_same_check_spelled_differently_runs_once() {
        let c = |cmd: &str| Check {
            command: cmd.into(),
            dir: "backend".into(),
            reason: String::new(),
            tests: false,
        };
        assert!(same_check(
            &c("cargo fmt --all -- --check"),
            &c("cargo fmt --check --all")
        ));
        assert!(!same_check(&c("cargo test -p a"), &c("cargo test -p b")));
    }

    #[test]
    fn a_lockfile_change_tests_the_workspace_once() {
        let p = plan(
            &project(),
            &["Cargo.lock".into(), "crates/yuzu/src/a.rs".into()],
            false,
            false,
        );
        assert_eq!(commands(&p), ["cargo test --workspace"]);
    }

    #[test]
    fn a_leaf_crate_tests_only_itself() {
        let p = plan(
            &project(),
            &["crates/yuzu/src/wait.rs".into()],
            false,
            false,
        );
        assert_eq!(commands(&p), ["cargo test -p mochi-yuzu"]);
    }

    #[test]
    fn npm_packages_get_their_test_and_lint_scripts() {
        let p = plan(&project(), &["web/src/App.tsx".into()], false, false);
        assert_eq!(commands(&p), ["pnpm test", "pnpm run lint"]);
        assert_eq!(p.checks[0].dir, "web");
    }

    #[test]
    fn an_unmatched_file_falls_back_to_the_full_suite() {
        let p = plan(
            &project(),
            &["crates/yuzu/src/a.rs".into(), "scripts/deploy.sh".into()],
            false,
            false,
        );
        assert!(p.full);
        assert_eq!(
            commands(&p),
            ["cargo test -p mochi-yuzu", "cargo xtask check"]
        );
        assert!(
            p.checks[1]
                .reason
                .contains("no check is linked to scripts/deploy.sh")
        );
        assert_eq!(p.unmatched, ["scripts/deploy.sh"]);
    }

    #[test]
    fn docs_go_to_drift_when_matcha_is_installed() {
        let p = plan(&project(), &["docs/a.md".into()], false, true);
        assert_eq!(commands(&p), ["matcha drift --in docs/a.md"]);
        let p = plan(
            &project(),
            &["docs/a.md".into(), "README.md".into()],
            false,
            true,
        );
        assert_eq!(commands(&p), ["matcha drift --in docs/a.md --in README.md"]);
        assert_eq!(p.checks[0].reason, "2 docs changed");
        let p = plan(&project(), &["docs/a.md".into()], false, false);
        assert!(p.full);
    }

    #[test]
    fn go_and_python_packages() {
        let mut pr = Project {
            go: vec!["svc".into()],
            python: vec!["tools".into()],
            ..Project::default()
        };
        pr.make_targets = vec!["test".into()];
        let p = plan(
            &pr,
            &["svc/internal/auth/token.go".into(), "tools/lib/a.py".into()],
            false,
            false,
        );
        assert_eq!(
            commands(&p),
            ["go test ./internal/auth", "python -m pytest"]
        );
        assert_eq!(p.checks[0].dir, "svc");
        // Both run tests, so mutate uses them.
        assert!(p.checks.iter().all(|c| c.tests));
        let p = plan(&pr, &["svc/go.mod".into()], false, false);
        assert_eq!(commands(&p), ["go test ./..."]);
        assert!(p.checks[0].tests);
        let p = plan(&pr, &["README"].map(String::from), false, false);
        assert_eq!(commands(&p), ["make test"]);
    }
}
