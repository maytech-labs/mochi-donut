# 0035. Merge pull requests when CI passes

- **Status:** Accepted
- **Date:** 2026-10-14

## Context

The owner wants pull requests to merge as soon as CI passes. GitHub's own auto-merge waits on branch protection, and this repo is private on the free plan, where rulesets and branch protection aren't available (403). Without them, `gh pr merge --auto` merges at once, before CI finishes. The repo merges by rebase, so each commit lands on `main` as reviewed ([ADR 0006](0006-commit-messages.md)).

## Decision

- **A workflow, `.github/workflows/merge-on-green.yml`,** runs when CI completes and rebase-merges the pull request, then deletes its branch. It follows Anchor's Dependabot workflow: no checkout, so no PR code runs with its write token.
- **Which PRs:** from a branch in this repo (not a fork), opened by the repo's owner or Dependabot, and not a draft. Marking a PR as a draft keeps it open.
- **Only what CI tested:** the PR's head must be the commit CI ran on (`--match-head-commit`), and the branch must not be behind `main`, so the rebased commits are the tree CI passed. A PR behind `main` is skipped with a notice; rebasing it runs CI again, which comes back here.
- **This isn't `taro land`.** taro still only reports ([ADR 0030](0030-taro.md)); merging follows the owner's rule, in CI.

## Consequences

- A merge made with `GITHUB_TOKEN` doesn't start CI on `main`. Because only up-to-date branches merge, `main` ends at the tree CI already passed.
- Two PRs open at once: the second is behind once the first merges, and needs a rebase before it merges.
- Dependabot rebases its PRs only on conflicts, so one behind `main` waits for a rebase (`@dependabot rebase`).
- Nothing waits for review. A PR the owner wants to look at again should be a draft until then.

## Alternatives considered

- **GitHub auto-merge:** needs branch protection, which needs GitHub Pro or a public repo.
- **Merge when behind, letting GitHub rebase:** the merged tree would be one CI never ran.
- **Update the branch from the workflow:** commits pushed with `GITHUB_TOKEN` don't start CI, so the PR would never go green again.
