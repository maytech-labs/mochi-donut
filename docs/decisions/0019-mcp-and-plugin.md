# 0019. MCP as `<tool> mcp` over the CLI, and a two-plugin marketplace in this repo

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

The design's phase 5 is the discovery layers: an MCP server as a thin layer over the CLI, with tool descriptions written as triggers; a short trigger table for a repo's agent instructions; and a Claude Code skill. It leaves open where the server lives, how it calls the commands, and how the skill and hooks reach a user.

## Decision

- **Each binary serves its own tools:** `matcha mcp` and `yuzu mcp`, MCP over stdio (newline-delimited JSON-RPC). One server per tool keeps the two-tool split from ADR 0003 and the design's packaging section.
- **A thin layer, as the design says:** a tool call maps its arguments to a command line and runs the same binary again with `--json` first, returning what it printed. The CLI stays the one implementation and the one thing tested. Exit 0 and 1 are both answers (1 means a check found something); exit 2 comes back as a tool error with the message. `--json` goes before the arguments, because after `yuzu check --` it would be read as part of the command being checked. The integration test caught exactly that.
- **The protocol code is shared** in `mochi_common::mcp`: `initialize` (echoing the client's protocol version when it's one of 2025-11-25, 2025-06-18, 2025-03-26, or 2024-11-05, else the newest), `ping`, `tools/list`, and `tools/call`. No resources or prompts. No new dependencies; matcha is 1,910 KB and yuzu 968 KB.
- **Tools:** matcha has `census`, `recipe_like`, `recipe_check`, `drift`, `ledger_add`, `ledger_verify`, `ledger_list`, `ledger_check`, and `ledger_summary`, each with an optional `repo`. yuzu has `status`, `check`, and `probe`. Not the hooks: safety checks run as hooks, never on the agent's say-so. Every description starts with "Use ..." and says when, not what.
- **This repo is a Claude Code plugin marketplace** (`.claude-plugin/marketplace.json`) with two plugins under `plugins/`:
  - `mochi-donut`: both MCP servers and a skill per tool. The skills hold the "when to use what" tables and how to read results. About 232 tokens are always in context.
  - `mochi-donut-hooks`: yuzu's `PreToolUse` check, with the fallback from ADR 0014, and matcha's `PostToolUse` recorder from ADR 0017.
  The hooks are a separate plugin because they change how commands get approved. Installing it is the user's deliberate act, which is what ADR 0014 asks for. Agents shouldn't install it.
- **The plugins call `matcha` and `yuzu` on `PATH`**, installed by Homebrew, `make install`, or Cargo. They don't bundle binaries.
- **The trigger table is in the README** for users to copy into their repo's `AGENTS.md` or `CLAUDE.md`.

## Consequences

- An agent in Claude Code sees the tools every session, and the skills load only when a task matches them.
- MCP clients other than Claude Code get the same tools from `matcha mcp` and `yuzu mcp`.
- Each tool call starts a process: a few milliseconds plus the command's own time.
- The design's phase-5 test, both tools on a second repo, is partly done: matcha has been measured on Ghostty, vim, MoltenVK, Ghidra, and IntelliJ (ADRs 0012, 0015), yuzu only on this Mac. Whether agents reach for the tools unprompted (the ledger's bar) needs sessions with the plugin installed.

## Alternatives considered

- **One `mochi mcp` server for both tools:** one entry to configure, but a third binary, or one tool serving the other's commands, against the packaging decision.
- **Calling the command code in-process instead of rerunning the binary:** faster, but every command would need a second, non-printing entry point, and the CLI would stop being the single tested path.
- **An MCP SDK crate:** more of the protocol for free, but most of it unused, and a dependency for about 200 lines.
- **Hooks in the main plugin:** one install, but anyone wanting only the tools would also change how their commands are approved.
