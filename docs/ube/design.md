# ube design

<!-- matcha: links-only -->
<!-- Its examples name files in other repos. -->

- **Status:** Built (2026-10-13). Proposed in [ADR 0028](../decisions/0028-propose-ube-and-taro.md); how it was built, and where it differs from this design, is in [ADR 0029](../decisions/0029-ube.md).
- **Date:** 2026-10-13

## Summary

ube knows **the change in hand**: what's different in the working tree, which of the repo's checks apply to it, whether they've run on exactly this code, and what failed, in a few lines. It answers "is this change done, and can I prove it?"

| Command | Question |
|---|---|
| `ube plan` | Which checks apply to this change? |
| `ube run` | Run them: what failed, in a few lines? |
| `ube done` | Is this change done, with evidence? |
| `ube mutate` | Can the tests for this change fail at all? |

## Background

Each of these happened in building mochi-donut itself:

- **"Done" before the checks that matter ran.** Features were declared working after unit tests passed. An audit later found many user-visible behaviors that no integration test covered ([ADR 0026](../decisions/0026-two-kinds-of-test.md)). Nothing tied "these files changed" to "these tests prove them."
- **Tests that pass on the first try prove little.** Every new integration test passed immediately. Breaking five behaviors on purpose showed they could fail, but that was done by hand, in an ad hoc loop whose first version silently reported nothing.
- **Failure output buries the failure.** A failing `cargo test` or CI log runs to hundreds of lines; the agent's context fills with them, and the one line that matters is easy to miss.
- **Checks ran on stale code.** It's easy to run the suite, edit a file, and then report the earlier green run. Nothing records which tree a result belongs to.

## Goals

- Name the checks a change needs, with the reason for each, from what the repo already declares.
- Run them and report failures compactly, with the command to rerun each.
- Tie every result to the exact tree it ran on, and record it in matcha's ledger as evidence.
- Show whether the tests touching a change can fail.

## Non-goals

- Replacing CI. CI stays the authority; ube is how an agent gets there faster and with proof.
- A new test runner or framework. ube runs the repo's own commands.
- Inventing checks. It only runs what the repo declares.

## Design

### Packaging: its own tool

ube **runs the repo's code**: test suites, builds, linters. matcha only reads, and stays fast and safe to call anywhere. yuzu's check sits in front of every command and must stay small. A tool that executes long, project-defined commands belongs on its own, with its own trust boundary. It shares `mochi-index` with matcha for history, and its commands are still checked by yuzu's hook like any other.

### `ube plan`: which checks apply?

- **Declared checks:** the repo's own commands, from CI workflows (`run:` steps in `.github/workflows/*.yml`, `.gitlab-ci.yml`), the `Makefile`, `package.json` scripts, `cargo xtask`, `justfile`, and an optional `.ube.toml`.
- **Selection:** for each changed file, the checks whose scope covers it (CI path filters, a workspace member, a package directory), and the test files that historically change with it (recipe's co-change statistics, applied to tests).
- **Output:** each check with its command, its reason ("`crates/yuzu/src/` changed; CI runs `cargo test -p mochi-yuzu` on it"), and what was left out and why. When it can't tell what applies, it says so and plans the full suite.

### `ube run`: what failed, in a few lines?

- Runs the plan, or `ube run -- <command>` for one command.
- Parses failures from common runners (cargo test and clippy, pytest, go test, jest and vitest, tsc, eslint), with a generic fallback (lines that look like `path:line: error`). Each failure becomes: the test or file and line, the message, and the command that reruns just it.
- Records the result against the tree's hash, and as a verified ledger entry with the command and summary as evidence.
- Exit 0 when everything passed, 1 when something failed.

### `ube done`: is this change done?

A checklist for the current tree:

- the planned checks ran green **on this exact tree**
- changed source files have matching test changes, or tests that cover them (per [ADR 0026](../decisions/0026-two-kinds-of-test.md))
- `matcha recipe --check` finds no missing companion file
- `matcha drift` finds no doc this change made false
- `matcha ledger check --diff` finds no assumed claim written into a doc

Exit 0 when all hold, 1 with the list of what doesn't.

### `ube mutate`: can these tests fail?

- Makes small changes to **changed lines only**, one at a time: flip a comparison, negate a condition, return early, replace a constant. Then runs the tests `plan` picked.
- A mutation that no test notices is a **survivor**: the line it changed isn't proven by any test.
- Uses an existing mutation tool when one fits the language (cargo-mutants, Stryker, mutmut), and its own line-based mutations otherwise.
- Always restores the tree. It works in a temporary copy, never in place.

### Interfaces

- **CLI** first, `--json` and the shared exit codes.
- **MCP tools:** `plan`, `run`, `done`, with descriptions that trigger on "before you say it's done".
- **A hook:** optionally, a Claude Code `Stop` hook that runs `ube done` when the agent ends its turn, and tells it what's missing.

### Data

Results go in `.ube/` (self-ignoring, like `.matcha/`): per tree hash, the checks run, their outcome, and parsed failures. Nothing leaves the machine.

## Status (2026-10-13)

All four commands are built. Differences from the design above:

- **No `.ube.toml`.** Checks come only from what the repo declares; nothing so far needed more.
- **Selection is by package**, not by individual test file: the package that holds a file, plus every Cargo package that depends on it through a path dependency. Picking test files from history (recipe's statistics) isn't built.
- **`mutate` uses its own line-based mutations** (comparison and boolean operators, arithmetic, `true`/`false`), not cargo-mutants or Stryker.
- **The Stop hook is built but not in the hooks plugin.** Whether it helps or nags is the trial's question.

Tried on this repo: a one-line yuzu change plans `cargo test -p mochi-yuzu` and leaves CI's full check out, saying so; a change to `mochi-common` plans it and every package that depends on it. `ube run` took 21 s, recorded the green run as ledger evidence, and `done` then caught an edit made after it. `ube mutate` on its own `plan.rs` found a line no test proved (a check's `tests` flag), which a test now pins.

## Done when

1. **Plan:** for the last 20 merged PRs of Anchor, this repo, and one other repo, the checks `ube plan` selects would have caught every CI failure those PRs hit, while running at most half the full suite's time on average.
2. **Run:** for a failing suite in cargo, pytest, and vitest, the report fits in 40 lines and names every failing test with a rerun command.
3. **Mutate:** on this repo's last 10 feature PRs, survivors point at lines a reviewer agrees are untested.
4. **Use:** in a two-week trial, measured by `yuzu sessions`, agents run `ube done` or `ube run` before saying they're done in most sessions.

## Risks

- **Test selection misses.** A skipped test that would have failed is false confidence, the worst outcome for this project. Mitigation: always list what wasn't run; plan the full suite when unsure; `done` says "selected checks", never "all checks".
- **Running arbitrary commands.** ube runs only commands the repo declares, and yuzu's hook checks them.
- **Parsers drift** as test runners change their output. Mitigation: the generic fallback, and evals of real failure logs pinned by `matcha eval`.

## Alternatives considered

- **Extend matcha:** matcha would start executing code, losing its read-only, call-anywhere guarantee.
- **Leave it to the agent:** that's today, and the background section is what it produces.
- **Bazel, Nx, or Turborepo-style affected-target tools:** precise, but only in repos built with them.

## Open questions

1. Should `.ube.toml` exist at all, or should everything come from what the repo already declares?
2. Is the `Stop` hook helpful or nagging? A trial decides.
3. Mutation testing is slow. Is changed-lines-only fast enough to be routine, or is `mutate` an occasional check?
