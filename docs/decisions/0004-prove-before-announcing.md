# 0004. Prove the tools are useful before announcing them

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

Six capabilities are a lot to build, and some are less proven than others. The evidence so far comes from a single working session. A tool that sounds sure but is wrong is worse than no tool.

## Decision

The repo can be public from the start, but nothing is announced or published to crates.io until:

- `matcha census` and `matcha recipe` meet their "done when" bars (design doc) on Anchor and on one other repo with a different language and history, and
- `yuzu check` has run as a pre-command hook in the owner's real sessions for at least two weeks without blocking a legitimate command it shouldn't have.

`drift`, `ledger`, and `probe` ship later, each when it meets its own bar.

## Consequences

- The first release is small but trustworthy.
- If a tool misses its bar, we change course or drop it, and record that in an ADR.

## Alternatives considered

- **Release early and iterate in public.** Faster feedback, but a noisy or wrong first impression is hard to undo for a tool whose whole value is being trustworthy.
