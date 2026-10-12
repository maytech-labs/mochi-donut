# 0006. Terse conventional commits, no tool attribution

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

The history is public and read by people and tools: changelogs, release notes, and `matcha recipe`, which learns from commits. The owner wants a short, uniform history whose author is the person who owns each change, whatever tools helped write it.

## Decision

- Commit messages follow Conventional Commits 1.0: `type(scope): subject`, lowercase, imperative, no period, about 50 characters at most.
- Types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`, `chore`. Scopes are crate names or left off.
- Subject only, unless the why isn't obvious; then a body of a line or two.
- No tool attribution in commits or PR descriptions: no `Co-Authored-By` trailers for AI tools, no "Generated with" lines.

## Consequences

- The history scans quickly, and a changelog can be generated from it later.
- Context that doesn't fit a terse subject lives in the PR description, the ADRs, and the design doc.
- Agents working here must override any default attribution their tools add.

## Alternatives considered

- **Free-form imperative subjects** (as in many projects): readable, but no machine-readable type for changelogs or release tooling.
- **Attribution trailers for AI tools:** some projects want them for transparency; the owner doesn't for this repo.
