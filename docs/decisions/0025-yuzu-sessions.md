# 0025. yuzu sessions: tool use from agent transcripts, and a log of the hook's asks and denies

- **Status:** Accepted
- **Date:** 2026-10-13

## Context

ADR 0004 judges the tools by use: whether `yuzu check` blocked a legitimate command in two weeks of real sessions, and whether agents reach for the tools unprompted (the ledger's bar, and the phase-5 question). Both live in agent transcripts and in what the hook decided, and nothing read either.

## Decision

- **`yuzu sessions`** reads Claude Code transcripts (`projects/*/*.jsonl` under `$CLAUDE_CONFIG_DIR` and every `~/.claude*`, compared by real path so none counts twice, or `--dir`) and counts, for each `--tool` (default `matcha` and `yuzu`): sessions using it, calls, calls by subcommand or `mcp:<tool>`, calls by project, and **unprompted** calls, made before the user mentioned the tool in that session.
- **A CLI call** is found with yuzu's own shell splitter, so a tool name inside a heredoc or a path isn't a call; wrappers like `env` and `timeout` are looked through. **An MCP call** is a `mcp__<server>__<tool>` name whose server is the tool or ends in `_<tool>` or `-<tool>`.
- **A mention** is the tool's name in text the user typed. Injected blocks (text starting with `<`) and `isMeta` entries don't count, so a system reminder or a loaded `CLAUDE.md` doesn't make a call look prompted.
- **`--since`** (default 14d), **`--project`** to keep one tree, **`--exclude`** to leave one out, such as the tool's own repo while it's built.
- **The hook logs its asks and denies** to `$XDG_STATE_HOME/yuzu/decisions.jsonl` (default `~/.local/state/yuzu/`): the time, decision, command, directory, and the reason's first line. Allows aren't logged. `YUZU_NO_LOG=1` turns it off. Logging can't change a decision. `sessions` shows the counts and the last ten, to review for wrong blocks. This amends ADR 0014.
- Local files only; no network.

## Results (2026-10-13)

Over 7 days of this Mac's sessions, excluding this repo: matcha was called 7 times in 2 of 17 sessions, once unprompted (the headless plugin test); yuzu 7 times, none unprompted. Including this repo, 84 of matcha's 98 calls were building it. This is the baseline before the plugin and hooks are installed.

## Consequences

- ADR 0004's two-week trial has its numbers: unprompted use per tool, and every ask and deny to judge.
- Only Claude Code's transcript format is read. Another agent needs its own reader; the counting doesn't change.
- The decision log holds commands, locally, in plain text.

## Alternatives considered

- **Count from the hook alone:** sees every Bash command, but not MCP calls, and not whether the user asked for the tool.
- **Log allowed commands too:** a complete record, but a copy of every command an agent runs, for little gain over the transcripts.
