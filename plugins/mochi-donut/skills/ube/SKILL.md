---
name: ube
description: Use when testing a change, and always before saying a change is done or working - which of the repo's own checks apply to the changed files, running them with failures reported in a few lines, a done checklist for the exact current code, and whether the change's tests can fail at all.
---

# ube: the change in hand

ube knows which of the repo's own checks (Cargo packages and their dependents, npm scripts, Go packages, pytest, the Makefile, CI) apply to what changed, and keeps each result tied to the exact code it ran on. Use the MCP tools when they're available (`plan`, `run`, `done`); otherwise run `ube <command> --json`. `ube agent-guide` prints the full guide.

## When to use what

| Moment | Tool | What you get |
|---|---|---|
| Before testing | `plan` | The checks that apply, why each, and what's left out |
| To test | `run` | Each failure in a few lines, with a command that reruns just it |
| After fixing a failure | `run` with `rerun_failed: true` | Just what failed in the last run |
| Before saying it's done | `done` | Checks passed on this exact code; tests changed; no missing companion, no doc made false, no assumed claim in a doc |
| To see whether the tests prove the change | `ube mutate` (CLI) | Lines whose mutation no test notices |

## Rules

- Don't say a change works or is done until `done` passes, or you've told the user plainly which items it reports as failing or unknown.
- Results belong to the exact code they ran on. After any edit, run again before relying on them.
- `plan` lists what it left out (usually CI's full suite). Say so when you report results, or run `run` with `all: true`.
- Prefer `run` over running test commands yourself: its output is short and keeps your context for the work.
