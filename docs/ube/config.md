# Configuring ube

<!-- matcha: links-only -->
<!-- The tables name files other kinds of repos have, so `matcha drift` checks only this page's links. -->

ube has no configuration file. It reads the checks a repo already declares.

## How checks are found

From tracked files, and new files not yet added:

| Source | Checks | Applies to a changed file when |
|---|---|---|
| `Cargo.toml` packages | `cargo test -p <name>` | The file is in the package, or in a package it depends on through a `path` dependency, written in the package or inherited from `[workspace.dependencies]` |
| fmt and clippy in CI `run:` steps, the `Makefile`, `justfile`, `xtask/`, or `.cargo/config.toml` | `cargo fmt --check -p <name>`, `cargo clippy -p <name> --all-targets`, with `-- -D warnings` when one of them denies warnings | A `.rs` file in the package changed. For a workspace-wide change, `--all` and `--workspace` |
| `Cargo.lock`, the workspace `Cargo.toml`, `rust-toolchain.toml` | `cargo test --workspace` | Always: they affect every package |
| `package.json` | `<manager> test`, and `run lint`, `run typecheck`, `run check` when defined | The file is under the package; the manager comes from the nearest lockfile (`pnpm-lock.yaml`, `yarn.lock`, `bun.lock`, else npm) |
| `go.mod` | `go test ./<package>`, or `go test ./...` for `go.mod` and `go.sum` | The file is a `.go` file in the module |
| `pyproject.toml`, `pytest.ini`, `setup.cfg`, `tox.ini`, `setup.py` | `python -m pytest` | The file is a `.py` file in the project |
| Docs (`.md`, `.mdx`) | `matcha drift --in <file>` | matcha is installed |
| CI (`.github/workflows/*.yml`, `.gitlab-ci.yml`) | Its test, lint, and check commands | The full suite: `--all`, or a changed file nothing else covers |
| The root `Makefile` | `make check`, else `make test` | The full suite, when CI declares none |

## What's under `.ube/`

| Path | Holds |
|---|---|
| `.ube/.gitignore` | `*`, so the folder never reaches a commit |
| `.ube/results/<tree>.json` | Each check's result on that tree: passed, exit code, time, parsed failures |
| `.ube/last-failed.json` | The checks that failed in the last run, for `ube run --rerun-failed` |

`<tree>` is the id of a git tree holding the working tree exactly as it was, untracked files included. It's written through a temporary index, so the real staging area isn't touched.
