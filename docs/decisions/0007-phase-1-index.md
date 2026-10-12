# 0007. Phase 1 index: ask git, compute in memory

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

`census` and `recipe` need the tracked files and the commit history. The design describes a persistent, incremental index under `.matcha/`. Building that first would delay finding out whether the tools are useful at all.

## Decision

- `mochi-index` runs the `git` command (`ls-files`, `log --name-only`, `diff --name-only`, `show`) and parses its output. No git library.
- Everything is computed in memory on each run. No `.matcha/` cache yet.
- History is limited to the last 2000 non-merge commits by default (`--history`), renames are not followed, and commits that changed more than 300 files are ignored (`--max-files`).
- `census` reads tracked text files only: under 1 MiB, no NUL byte in the first 8 KiB.

## Consequences

- No native dependency, and a smaller binary: `git` is already on every machine these tools run on.
- Fast enough for now: `recipe --check` against 2000 commits of Ghostty's history takes about 65 ms, and `census` over Anchor's 512 files about 0.1 s of CPU.
- A renamed file starts a new history. Revisit if `--replay` shows renames costing catches.
- Add the persistent index when a repo is big enough that a run takes over a second.

## Alternatives considered

- **`gix` or `git2`:** no subprocess, but a much larger binary (and a C dependency for `git2`), for no measured gain yet.
- **The persistent index now:** the right end state for huge repos, but premature before the tools prove useful (ADR 0004).
