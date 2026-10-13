# taro design

<!-- matcha: links-only -->
<!-- Its examples name files and settings in other repos. -->

- **Status:** Built (2026-10-13). Proposed in [ADR 0028](../decisions/0028-propose-ube-and-taro.md); how it was built, and where it differs from this design, is in [ADR 0030](../decisions/0030-taro.md).
- **Date:** 2026-10-13

## Summary

taro knows **the forge and the team**: the pull request, its reviews and checks, who owns the code, how the repo merges, and which branches are really done. It answers "what does this change need before it can merge, and what's safe to clean up after?"

| Command | Question |
|---|---|
| `taro ready` | Can this PR merge? If not, what's blocking it? |
| `taro reviews` | What review feedback is still open? |
| `taro owners` | Who owns these files, and who should review? |
| `taro flaky` | Which checks fail intermittently? |
| `taro cleanup` | Which branches are merged and safe to delete? |

## Background

From building and merging mochi-donut:

- **Merging took judgment nothing encoded.** Which merge method keeps the repo's history (rebase, here, to keep one change per commit), whether stacked branches were contained in the top one, whether CI was green on the final commit.
- **Cleanup needed proof by hand.** After a rebase merge, git's own "merged" check didn't recognize the branches. Proving their content had landed meant comparing trees and checking ancestry for each.
- **Forge state was scattered.** Auth, checks, PR state, and merge settings each took a different `gh` call, and a stale token broke them all. yuzu now answers who you are (`whoami`) and when CI is done (`wait`); what the PR needs is still assembled by hand.
- **Team repos add more:** unresolved review threads, required approvals, CODEOWNERS, flaky checks that get rerun until green. A solo repo shows little of this; the design has to be tried on team repos.

## Goals

- One answer to "can this merge?", with every blocking reason and what to do about each.
- Review feedback as a checklist an agent can work through.
- Reviewers suggested from ownership and history.
- Flaky checks named from CI history, with their rates.
- Branch cleanup that proves content landed, whatever the merge method.

## Non-goals

- Merging or deleting on the agent's own judgment. taro reports; a person (or an explicitly asked agent) acts, and yuzu's hook still checks the commands.
- A forge client. taro composes `gh` and `glab` answers; it doesn't replace them.
- Code review itself.

## Design

### Packaging: its own tool

Everything taro does goes over the network to a forge, so it's slow and needs credentials. yuzu's check, which runs before every command, must stay local and fast, and matcha answers from the local repo alone. So taro is separate. It reuses yuzu's identity logic (moved to `mochi-common`) to explain auth failures the way `yuzu whoami` does. `yuzu wait ci` stays in yuzu: waiting is general, and taro can call it.

### Forges

GitHub (and GitHub Enterprise) through `gh api`, REST and GraphQL; GitLab through `glab api`. Each is an adapter behind the same questions. The forge and repo come from the push remote, as `yuzu wait ci` finds them.

### `taro ready [PR]`

Checks, for the PR (default: the current branch's):

- **Checks:** required ones (from branch protection or rulesets) passed on the head commit
- **Reviews:** approvals the rules require, no outstanding "changes requested", no unresolved threads
- **Mergeability:** no conflicts; how far behind the base it is
- **The repo's own conventions, learned from its history:** commit subjects in its usual form (Conventional Commits here), a changelog entry when its PRs usually add one, an ADR when its PRs usually do. "Usually" is measured the way `matcha census` and `recipe` do, with the counts shown.

Exit 0 when ready, 1 with each blocker and what to do.

### `taro reviews [PR]`

Unresolved threads as a checklist: file and line, the reviewer, the latest comment, and whether the code at that line changed since. `--json` for agents working through them.

### `taro owners [PATHS | --diff]`

CODEOWNERS for the paths, plus who changed them most in the last year (from history), as suggested reviewers.

### `taro flaky [--since 30d]`

From CI history: checks that failed and then passed on the same commit, with their rate and last occurrence. Answers "is this failure mine?" before an agent starts debugging a flake.

### `taro cleanup`

Lists local and remote branches whose content is in the base branch, proved by ancestry, by an identical tree, or by every commit's patch being present (`git cherry`), which covers merge, rebase, and squash. Also lists branches that are **not** safe to delete, and why. Deleting is a separate, explicit `taro cleanup --delete`, which asks first.

### Interfaces

- **CLI** first, `--json`, shared exit codes.
- **MCP tools:** `ready`, `reviews`, `owners`, `flaky`.
- **No hooks:** everything taro does is a network call.

### Data

Nothing stored, except an optional short-lived cache of CI history for `flaky`. Credentials stay with `gh` and `glab`.

## Status (2026-10-13)

All five commands are built. Differences from the design above:

- **GitHub only.** GitLab says it isn't supported yet, as `yuzu wait ci` does.
- **Auth failures point at `yuzu whoami --verify`** instead of sharing its code.
- **Conventions need 10 merged PRs**, and block only when at least 90% of them hold (warn from 60%).
- **When required checks can't be read** (rulesets and branch protection return 403 on a free private repo), every check counts, and the output says so.
- **No `taro land`**: taro reports, and merging stays with the user.

Tried on this repo: `taro ready 6` saw two passing checks and too few merged PRs to learn conventions from; `taro cleanup --remote` kept `feat/ube` and the open Dependabot branch, naming the commits not in `main`. Fixtures merged by merge commit, rebase, and squash are each proved, and a branch half cherry-picked is kept.

## Done when

1. **Ready:** on the last 30 merged PRs of two team repos, the blockers `taro ready` would have listed before merge match what reviewers actually required, in at least 95% of PRs.
2. **Cleanup:** across fixture repos merged by merge, rebase, and squash, `taro cleanup` never calls a branch with unmerged content safe, and finds every merged one.
3. **Flaky:** on a repo with known flaky checks, it names them, and names no check that only ever fails for real.
4. **Use:** in a trial, agents use `taro ready` instead of hand-assembled `gh` calls before asking to merge.

## Risks

- **Rate limits and slow APIs.** Mitigation: GraphQL where one query answers many questions, and the CI-history cache.
- **Conventions learned wrong.** "Usually adds a changelog" from a small or mixed history is a guess. Mitigation: show counts, and stay quiet below a sample size, as recipe does.
- **Deleting the wrong branch.** Mitigation: proof before listing, a separate explicit flag to delete, and yuzu's hook on the push.
- **Solo repos make it look thin.** Mitigation: the bars require team repos.

## Alternatives considered

- **Put it in yuzu:** it would bring network calls and forge APIs next to the check that runs before every command.
- **Use `gh` directly:** that's today. Each question takes several calls, and none of them knows the repo's conventions.
- **Forge bots** (merge queues, review assignment): powerful, but configured per org, and invisible to an agent working locally.

## Open questions

1. Should `taro land` exist (merge by the repo's method, wait for checks, then clean up), or does that cross from reporting into acting?
2. Can conventions be learned well from history, or does a repo need to declare them?
3. Which team repos are available to prove it on?
