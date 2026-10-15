# 0037. `yuzu env add` declares a target, and asks name it

- **Status:** Accepted
- **Date:** 2026-10-15

## Context

When `check` asks about an undeclared target, the usual fix is to declare it. The message said to "add it to ssh_hosts under an env with class = \"mine\"", which left the user to find the file, the env, and the TOML syntax. The user asked for the prompt itself to offer "approve, deny, or add this env". Claude Code draws that prompt: a `PreToolUse` hook can only answer allow, deny, or ask, with a reason. A third button isn't possible from yuzu.

## Decision

- `yuzu env add <env> <key> <pattern>...` adds patterns to one env's list. A new env needs `--class`. An existing env with a different class is an error, not a silent change. Patterns already there are reported, not repeated.
- It edits the file in place with `toml_edit`, so the user's comments and layout survive, and it refuses to write a result that wouldn't load.
- When an earlier env still matches an added name first, the output says so (`shadowed` in JSON), since the add then changes nothing.
- Running it is a write to yuzu's config, so the hook asks ([ADR 0036](0036-yuzu-guards-its-config.md)). It isn't an MCP tool.

## Consequences

- Declaring a host is one line the user can run with `! yuzu env add ...`, or approve when an agent runs it.
- `toml_edit` (with `indexmap` and `toml_writer`) is a new dependency. The release binary went from 1,133,728 to 1,233,040 bytes (+97 KiB, 8.8%), new code included.
- The user still sees two prompts for a new host: one for the command, one for `env add`. That's the price of keeping the hook to allow, deny, or ask.

## Alternatives considered

- **Deny, and have the agent ask the user with its own three-option question.** The agent would then run `env add` itself, and an agent that can declare hosts without the user's approval can approve itself.
- **Serialize the file with `toml`.** It drops comments. The user's file has them.
- **Edit the text by hand.** Arrays can span lines and carry comments; a parser is less risky than string surgery.
- **`yuzu env add --ssh-host balin` flags per kind.** The positional key matches what the user sees in the file, and one form covers every kind.
