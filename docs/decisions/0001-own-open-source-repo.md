# 0001. An open-source monorepo of its own

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

The tools were first designed inside Anchor, a private app repo, because that's where the problems showed up. But the problems (agents guessing conventions, missing companion edits, trusting drifted docs, acting on the wrong environment) happen in every repo.

## Decision

- The tools live in their own repository, `mochi-donut`, published as open source.
- It's a monorepo: both tools, their shared crates, docs, and decisions in one place, released together.
- Anchor is the first test bed. The evaluations in the design doc run against Anchor's history and docs, and against at least one other repo before release ([0004](0004-prove-before-announcing.md)).

## Consequences

- The tools can't depend on Anchor's layout, conventions, or release cycle. Repo-specific knowledge goes in optional configuration.
- Anchor's rules (its docs, CI, and contract-test guards) don't apply here, so this repo carries its own `AGENTS.md`.
- Examples in the design doc come from Anchor, which is private; they're quoted, not linked.

## Alternatives considered

- **Keep the tools inside Anchor until they prove themselves.** Faster to start, but the tools would grow Anchor-specific assumptions that are hard to remove later.
- **A repo per tool.** The tools share conventions, an output format, and (for matcha) an index. Separate repos would duplicate all of that.
