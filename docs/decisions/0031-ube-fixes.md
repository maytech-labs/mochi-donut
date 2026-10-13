# 0031. ube measures a branch from where it started, and plans the Rust lints the repo runs

- **Status:** Accepted
- **Date:** 2026-10-13

## Context

Using ube on this repo after [ADR 0029](0029-ube.md) showed four ways it called a change proven when it wasn't:

- **Committed work vanished.** The default base was `HEAD`, so once an agent committed, `ube done` said "No changes against the base: nothing to prove" and exited 0. `--base main` compared against main's tip, so commits that landed on main after the branch started counted as the branch's change.
- **CI failed where ube passed.** A Rust change planned only `cargo test -p <pkg>`. CI here runs `cargo xtask check` (fmt, then clippy with `-D warnings`), so a misformatted or warning-producing change passed `ube done`.
- **Dependents were missed.** Path dependencies written `core = { workspace = true }` or `core.workspace = true`, with the path in `[workspace.dependencies]`, weren't read. That's the usual style in newer workspaces. The parser also took `pathdiff = "0.2"` for a path.
- **`--rerun-failed` did nothing after a fix.** It looked up failures under the current tree, and fixing the code makes a new tree.

## Decision

- **The base defaults to where the branch started**: the merge-base of `HEAD` and the repo's default branch (a remote's `HEAD`, else a local `main` or `master`), taking the candidate closest to `HEAD` so a stale local `main` doesn't add other people's commits. It's `HEAD` on the default branch, on a detached `HEAD`, and on a branch with no commits of its own. `--base REV` means `REV`'s merge-base with `HEAD`. `plan` and `done` print and return the base (`"base": {"name", "commit"}`, or `null` for `HEAD`). This lives in `mochi-index` as `RepoRoot::branch_base`.
- **Rust lints are planned when the repo runs them**: `cargo fmt --check` when a workflow's `run:` commands, the Makefile, justfile, `xtask/**/*.rs`, or `.cargo/config.toml` checks formatting, and `cargo clippy --all-targets` when one runs clippy, with `-- -D warnings` when one denies warnings. Comments and workflow setup (`components: clippy`) don't count. They cover the packages holding changed `.rs` files (not their dependents, whose tests already build them), or the workspace for a workspace-wide change. They're marked as not tests, so `mutate` doesn't run them. `cargo fmt --check` failures are parsed to one per file, with the command that fixes it.
- **Workspace-inherited path dependencies are resolved** against `[workspace.dependencies]`, and manifest entries are read as `key = value` with their fields, not by searching for `path`.
- **`--rerun-failed` reruns the checks that failed in the last run**, kept in `.ube/last-failed.json`, whatever tree they ran on. A run's results still count for `done` only on the tree they ran on.
- No new dependencies. ube is 1,746 KB, up from 1,730.

## Consequences

- An agent that commits before running `done` is still asked for proof, and so is the Stop hook.
- A one-package Rust change in this repo plans three checks instead of one (tests, fmt, clippy); fmt and clippy took under 2 s here.
- Lint detection is a text heuristic. A repo that runs clippy only through a script ube can't read still gets tests only, and the left-out full suite says so.
- `RepoRoot::branch_base` is available to matcha too; matcha doesn't use it yet.

## Alternatives considered

- **Keep `HEAD` and tell agents to pass `--base main`:** the agent that forgets is the one ube exists for.
- **Always plan fmt and clippy for Rust:** a repo that doesn't run them would fail on lints it never asked for. ube only runs what the repo declares ([ADR 0029](0029-ube.md)).
- **`.ube.toml` to declare lints:** still not needed; the repo's own scripts say it.
