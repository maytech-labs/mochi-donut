# 0027. A docs folder per tool, with the same pages in each

- **Status:** Accepted
- **Date:** 2026-10-13

## Context

All design lived in one `docs/design.md`, with usage spread across the README, the agent guides, and ADRs. As matcha and yuzu grew to eleven commands, and two more tools were proposed, one file mixed the cross-tool design with every command's detail, and there was no place for a command's full usage or configuration reference.

## Decision

- **`docs/<tool>/` for each tool:** `matcha`, `yuzu`, `ube`, `taro`.
- **The same pages in each:** `README.md` (what it is, commands at a glance), `design.md` (problem, how it works, bar, status, open questions), `usage.md` (every command and flag, examples, exit codes), `config.md` (configuration and files written, where any), `agents.md` (hooks, MCP, skill, trigger lines). Proposed tools start with `README.md`, `design.md`, and a planned `usage.md`.
- **`docs/design.md` keeps what crosses tools:** summary, background, goals, packaging, interfaces, discovery, rollout, risks, and alternatives. Its per-command sections moved to the tools' design docs, with their text unchanged apart from heading levels, links, and status updates.
- **`docs/README.md`** indexes it all.
- **Design docs are `links-only` for drift** (their examples quote other repos); usage, config, and agents pages are checked fully, so drift keeps them true.
- Code comments point at the page that describes them.

## Consequences

- A reader finds any tool's design, usage, or configuration in the same place.
- A new tool starts by copying the folder's shape.
- Links into the old `docs/design.md` sections from outside this repo break; inside it, drift checks every link.

## Alternatives considered

- **Keep one design doc and add usage pages beside it:** less moving, but the design doc keeps growing with every command.
- **Generated docs from `--help`:** always current, but `--help` can't hold design reasoning or examples. The usage pages were written from `--help` output, which found three undocumented arguments.
