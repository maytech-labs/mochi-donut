# 0008. How recipe decides a file is missing, and how we measure it

- **Status:** Accepted. Default `--min-support` changed to 6 by [0012](0012-recipe-support-six.md).
- **Date:** 2026-10-12

## Context

`recipe --check` says "Y usually changes with X but didn't". It's only worth running if it's usually right. The design set a bar of 90% precision and 80% recall on a replay of real commits.

The first replay on Anchor gave 10% precision. Two causes:

1. The metric counted the same wrong warning once per removed file (about 35 times per commit). A person runs the check once per change.
2. Files that change in nearly every commit (Anchor squash-merges whole features, so `AGENTS.md` and the registration files are in most commits) look like companions of everything.

## Decision

- **A companion** of X must have changed with X in at least `--min-confidence` (0.9) of at least `--min-support` (5) earlier changes to X, and have a **lift** of at least `--min-lift` (2): it must be at least twice as likely to change with X as in any commit. Lift filters out files that change in most commits anyway.
- **`recipe --replay N` measures it on the repo's own history.** For each of the last N commits, learning only from older commits: check the complete commit (anything flagged is a false alarm, counted once), then remove each file in turn and see whether `check` names it.
  - precision = caught / (caught + false alarms)
  - recall = caught / files removed
- **Recall over all files is a lower bound, not the design's bar.** Most files (a new route, a one-off fix) can't be predicted by anyone. The design's 80% recall target was meant for companion files; a fair denominator for that needs labeled data we don't have. Precision is the bar that matters for trust.

## Results (2026-10-12, defaults)

| Repo | Replayed | Caught | False alarms | Precision | Recall (all files) |
|---|---|---|---|---|---|
| Anchor (Rust and TypeScript, squash merges) | last 20 commits, 696 removals | 110 | 28 | 80% | 16% |
| Ghostty (Zig, 12k commits) | last 200 commits, 504 removals | 36 | 8 | 82% | 7% |

What it catches is the intended kind: in Ghostty, `build.zig.zon.nix` and `build.zig.zon.txt` (generated with `build.zig.zon`) and the release workflows; in Anchor, `openapi.json` and `schema.d.ts` (15 of 15) and the API registration files. Some "false alarms" are real rules the replay can't credit, such as `.env.example` when `config.rs` changed (5 of 5).

**Precision is below the 90% bar on both repos.** So `recipe --check` stays report-only (exit 1, but not wired to block commits) until it clears 90%. Ideas to try next, each measured with `--replay`: weight co-changes by commit size (a pair in a 3-file commit is stronger evidence than in a 60-file squash merge), favor recent history, and learn path patterns (`routes/*.rs` with `tests/contract/*.rs`).

## Consequences

- Any user can see how much to trust `recipe` on their own repo with one command, before relying on it.
- Thresholds are flags, so repos with unusual histories can tune them, and `--replay` shows the effect.

## Alternatives considered

- **Confidence and support alone:** 79% precision on Anchor at the same settings; lift 3 gives 85% but loses 15% of catches. Lift 2 removes two false alarms with no loss.
- **Leave-one-out with every warning counted:** overstates false alarms about 35 times for large commits.
