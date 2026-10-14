# 0034. taro counts every check when the rules require none

- **Status:** Accepted
- **Date:** 2026-10-14

## Context

The first live run of `taro ready`, on this repo's PR #9, printed "All 2 checks passed" while both were still running. The branch has no protection rules, so GitHub's `isRequired` is false for every check, and [ADR 0032](0032-taro-fixes.md) read that as "nothing is required": every check was optional, optional checks still running didn't block, and the fall-through message counted every check as passed.

## Decision

- **Rules that require no check say nothing about which checks matter.** When the required list is empty, every check counts, as when it can't be read, and the output says the rules require none. A check is optional only beside checks the rules do require.
- **An optional check that failed or is still running warns,** naming it; one still running points at `yuzu wait ci`. "All N checks passed" is printed only when all N did.

## Consequences

- On a repo with no branch protection, a running or failing CI check blocks `ready`, though GitHub would allow the merge. That matches what CI is for when nothing says otherwise, and fails safe.
- A repo that wants some checks to stay optional has to require the others.

## Alternatives considered

- **Keep empty as "nothing required", and only fix the message:** truthful, but `ready` would still say a PR is ready while its only CI runs.
