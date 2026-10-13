---
name: taro
description: Use before asking to merge a pull request or when asked whether one can merge, when addressing review feedback, when choosing reviewers, before debugging a CI failure (is it a known flake?), and after branches merge (which are safe to delete).
---

# taro: the forge and the team

taro answers from GitHub (through gh) and the local clone: what a PR needs before it can merge, open review threads, who owns files, which checks are flaky, and which branches are proved merged. Use the MCP tools when they're available (`ready`, `reviews`, `owners`, `flaky`); otherwise run `taro <command> --json`. `taro agent-guide` prints the full guide.

## When to use what

| Moment | Tool | What you get |
|---|---|---|
| Before asking to merge, or asked whether it can | `ready` | Every blocker with what to do: checks, reviews, threads, conflicts, draft, and the repo's conventions from its merged PRs |
| Addressing review feedback | `reviews` | Open threads with file, line, reviewer, latest comment, and whether the code changed since |
| Choosing reviewers | `owners` | CODEOWNERS for the change, and who changed those files most |
| A CI check failed | `flaky` | Whether that check is known to fail and then pass on the same commit |
| After merging | `taro cleanup` (CLI) | Branches proved merged, and the ones to keep, with why |

## Rules

- Report; don't act. Don't merge a PR or delete branches unless the user asks you to. `taro cleanup --delete` needs `--yes` from you, so only pass it when told to.
- Before debugging a failed check, run `flaky`: a known flake isn't your bug.
- If gh can't authenticate, run `yuzu whoami --verify` and tell the user the fix it gives.
