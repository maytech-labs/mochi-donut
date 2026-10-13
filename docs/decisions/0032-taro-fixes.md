# 0032. taro takes required checks from isRequired, and blocks when GitHub does

- **Status:** Accepted
- **Date:** 2026-10-13

## Context

Reviewing taro after [ADR 0030](0030-taro.md) against how GitHub actually answers found places where it called something safe that wasn't:

- **Red required checks could pass `ready`.** The rulesets endpoint answers `200 []` on a repo that uses classic branch protection, and the protection endpoint needs admin rights. That pair read as "nothing is required", so every failing check became an optional warning.
- **GitHub's own verdict was ignored.** `mergeStateStatus: BLOCKED` with nothing taro reads to explain it (signed commits, a deployment, a merge queue, a ruleset) still printed "Ready to merge". `BEHIND` only warned, but GitHub reports it only when the base requires branches to be up to date, so it does block.
- **`flaky` named things that aren't flakes.** A rerun of a run that had already passed counted, and so did a cancelled run (a manual cancel, or `cancel-in-progress` on a newer push) followed by a pass.
- **`cleanup` offered branches someone is working on.** A branch checked out in another worktree, even one with no commits yet, was "merged", and `--delete` then failed on it.
- **An invalid `GH_TOKEN` read as "No PR found"** when no PR number was given.

## Decision

- **Required checks come from GraphQL `isRequired(pullRequestNumber:)`** on each reported check run and status context. It covers rulesets and branch protection, and needs only read access. Rulesets and readable branch protection add required checks that haven't reported. Without `isRequired` (an older GitHub Enterprise), a ruleset list alone isn't an answer: it takes readable protection, and "Branch not protected" counts as one. Otherwise every check counts, as before, and the output says so. The same query returns review threads, so `ready` makes one GraphQL call.
- **`BEHIND` blocks**, with `gh pr update-branch <n>` as the fix. **`BLOCKED` with no other blocker** adds a `rules` item that blocks and points at `gh pr view <n> --web`.
- **`flaky` doesn't count cancels as failures**, and a rerun counts only when a job failed in an earlier attempt and passed in the last. The workflow-level fallback is only for when the attempts' jobs weren't read.
- **`cleanup` keeps branches checked out in any worktree**, naming the path. The same-content proof diffs with `--no-renames`, so a file the branch moved must be gone from the base too.
- **Auth errors pass through unwrapped**, and name `GH_TOKEN` when it's set, since gh uses it over stored logins for github.com. GraphQL `owner` and `name` go as strings (`-f`).

## Consequences

- `ready` errs toward blocking: an unexplained `BLOCKED` blocks even if GitHub would let an admin bypass it.
- These were checked against stub `gh` answers shaped like GitHub's, not a live team repo: `GH_TOKEN` was invalid in the session that made them. The design's bars still need team repos.

## Alternatives considered

- **Read required checks only from rulesets and protection, but treat `[]` as unknown:** safe, but every check would count on most repos, which makes optional checks block.
- **Trust `mergeStateStatus` alone:** it says blocked, not why, and the point of `ready` is to say what to do about each blocker.
