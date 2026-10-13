# 0012. recipe needs six earlier changes; commit weighting didn't help

- **Status:** Accepted, amended by [0021](0021-usability-from-trying-the-tools.md)
- **Date:** 2026-10-12

## Context

ADR 0008 left `recipe --check` at 80% precision on Anchor and 82% on Ghostty, below the 90% bar, and listed ideas to try. We tried them, and measured on two more repos so the defaults don't fit just two histories.

## Decision

- **Default `--min-support` is 6** (was 5). It raised precision on all four repos:

  | Repo (commits replayed) | Support 5 | Support 6 |
  |---|---|---|
  | Ghostty (200) | 82%, 36 caught | **100%**, 28 caught |
  | vim (200) | 90%, 56 caught | **97%**, 36 caught |
  | Anchor (20) | 80%, 110 caught | 85%, 90 caught |
  | MoltenVK (200) | 75%, 224 caught | 78%, 214 caught |

  Higher support (8, 10) loses more catches than it gains precision.
- **Confidence stays 0.9 and lift stays 2.** Confidence 0.95 helps MoltenVK (86%) but costs Ghostty a third of its catches.
- **No commit weighting.** It made precision worse or no better.
- **The 90% bar is per repo.** `--replay` is how a repo finds out whether it clears it. On Ghostty and vim, `--check` is ready to block a commit; on Anchor and MoltenVK it should only report.

## What didn't help

All measured with `--replay` on Anchor (20) and Ghostty (200):

- **Weighting by commit size**, as 1/log2(files + 1) or 1/(files − 1): 79% and 81%, or 78% and 77%.
- **Weighting recent commits more**, halving every 100 or 300 commits: Ghostty fell to 34% or 69%. Too little history keeps any weight, and old rules don't stop being true.
- **Requiring the reverse direction** (the companion's changes also include the file, at 0.5 or 0.7): Anchor up to 86 to 89%, at the cost of a third of Ghostty's catches.

## Why Anchor and MoltenVK stay below

- **Anchor** has 44 commits, nearly all squash-merged features of 30 to 54 files. History that short can't tell "this feature touched both files" from a rule. More history will help more than more scoring.
- **MoltenVK**'s false alarms are mostly a header without its implementation (`MVKDevice.h` without `MVKDevice.mm`, 177 of 190). That's a strong habit, not a rule.

## Consequences

- Fewer, surer warnings everywhere, and fewer catches.
- A pre-commit hook should block only where `--replay` shows 90% or better. Making `--check` read that itself is the next step if hooks need it.

## Alternatives considered

- **Support 8:** Ghostty stays at 100%, but vim drops to 17 catches and Anchor to 64, with no gain where it's needed.
- **Keep tuning for Anchor:** it's one young repo, and the cause is its history, not the scoring.
