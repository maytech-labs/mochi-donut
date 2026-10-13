# 0028. Propose ube (the change in hand) and taro (the forge and the team)

- **Status:** Accepted for ube, built per [0029](0029-ube.md); taro still proposed
- **Date:** 2026-10-13

## Context

Building and merging mochi-donut with its own tools showed friction neither matcha nor yuzu answers. Changes were called done before the checks that prove them ran, tests that passed on the first try had to be broken by hand to show they could fail, and failure output buried the failure. Merging took judgment about merge method and stacked branches, and cleanup after a rebase merge needed proof by hand that each branch had landed. The owner suggested two more mochi donut flavors, ube and taro, for tools ([ADR 0003](0003-names.md)).

## Decision

- **Propose `ube`**: the change in hand. `plan` (which checks apply), `run` (run them, failures in a few lines, recorded as ledger evidence), `done` (a checklist for the exact tree), `mutate` (can the change's tests fail). Separate because it runs the repo's code; matcha only reads. Design: [docs/ube/design.md](../ube/design.md).
- **Propose `taro`**: the forge and the team. `ready` (can this merge, and what blocks it), `reviews`, `owners`, `flaky`, `cleanup` (branches proved merged, whatever the merge method). Separate because everything it does is a network call; yuzu's check must stay local. Design: [docs/taro/design.md](../taro/design.md).
- **Not built until reviewed**, and each is proved against its "done when" bars before it ships, per [ADR 0004](0004-prove-before-announcing.md).
- **Ranked lower and not proposed:** the cost of adding a dependency (size, license), database safety (better as more of `yuzu check`), and memory between sessions (the ledger, ADRs, and agent memory cover it).

## Consequences

- Four tools instead of two: more for agents to discover, and more to maintain. Discovery stays layered; `yuzu sessions` will show whether agents reach for them.
- taro's bars need team repos, which mochi-donut and Anchor aren't.

## Alternatives considered

- **Fold them into matcha and yuzu:** fewer binaries, but matcha would execute code and yuzu would make network calls in the path of every command.
- **Don't build them:** keep working by hand. That's what produced the friction above.
