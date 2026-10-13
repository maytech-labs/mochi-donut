# Using ube

Every command takes `--json` (one JSON object on stdout) and `--repo <path>` (default: the repo containing the current directory). Exit codes:

| Code | Meaning |
|---|---|
| 0 | Nothing wrong: the checks passed, the change is done, every mutation was caught |
| 1 | A check failed, the change isn't done, or a mutation survived |
| 2 | Bad usage or no repo |

`ube agent-guide` prints a short version of this page, written for agents.

## plan

```sh
ube plan                  # this branch's changes, committed or not
ube plan --base v1.2      # everything since HEAD left v1.2
ube plan --all            # the full suite
```

The change is measured from where the branch started: the merge-base of `HEAD` and the default branch (a remote's `HEAD`, else `main` or `master`), plus uncommitted and untracked files. On the default branch, or a branch with no commits of its own, that's `HEAD`, so only uncommitted changes count. `--base <REV>` uses `REV`'s merge-base with `HEAD`, so commits that landed on `REV` later aren't counted. The output names the base, and `--json` has it as `base` (`null` for `HEAD`).

Lists each check with its command, the directory it runs in, and why it applies, then what the repo declares that the plan leaves out. How checks are found is in [config.md](config.md#how-checks-are-found). When a changed file has no check of its own, the plan is the full suite, and says which file caused that. Always exit 0.

## run

```sh
ube run                              # the plan
ube run --base main --all            # the full suite
ube run --rerun-failed               # only what failed in the last run, after a fix
ube run -- cargo test -p mochi-ube   # one command; one argument runs through sh -c
```

| Flag | |
|---|---|
| `--base <REV>`, `--all` | As for `plan` |
| `--rerun-failed` | Only the checks that failed in the last run, whatever has changed since |
| `--timeout <SECS>` | Stop a check after this long (default 1800) |

Progress goes to stderr. Each failure is reported in a few lines: the file and line, the test, the message, and a command that reruns just it, for cargo test, build, clippy, and fmt; pytest; go test; vitest and jest; tsc; and eslint. When no failure can be read from the output, its last lines are shown instead. Results are stored under the tree they ran on. A fully green run is recorded in matcha's ledger as a verified claim, when matcha is installed. If the checks changed files, the output says the code now differs from what they proved.

## done

```sh
ube done
ube done --base main
```

| Item | Passes when |
|---|---|
| `checks` | Every planned check passed on exactly this tree |
| `tests` | Every package with changed code has a test change too, in a test file or a Rust file's `#[cfg(test)]` module. A miss is a warning, not a failure. |
| `companions` | `matcha recipe --check` finds no missing companion file |
| `docs` | `matcha drift` finds no failing claim in a changed doc, or naming a changed file |
| `claims` | `matcha ledger check --diff` finds no assumed claim written into a doc or comment |

Without matcha, the last three are reported as unknown, not as passing. Exit 1 if any item fails.

## mutate

```sh
ube mutate
ube mutate --path crates/ube/src/plan.rs --max 5
```

| Flag | |
|---|---|
| `--base <REV>` | As for `plan` |
| `--path <PATH>` | Only mutate lines in these files or directories; repeatable |
| `--max <N>` | Mutations to try, spread over the change (default 20) |
| `--timeout <SECS>` | Per check run (default 600) |

Makes one small change at a time to added lines in non-test code (flipping `==`, `!=`, `<`, `>`, `<=`, `>=`, `&&`, `||`, `+`, `-`, and `true`/`false`, outside strings and comments), in a temporary copy of the repo, and runs the plan's test checks. A mutation no check notices is listed as a survivor, with its file, line, and before and after. Cargo builds reuse `target/ube-mutate`. If the checks fail before any mutation, it says so and stops. Exit 1 if any mutation survived.

## Serving agents

`ube mcp` serves `plan`, `run`, and `done` as MCP tools, and `ube hook claude-code` is the Stop hook. Both are described in [agents.md](agents.md).
