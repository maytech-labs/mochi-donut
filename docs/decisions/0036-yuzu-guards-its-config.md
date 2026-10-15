# 0036. yuzu asks before anything changes its own config

- **Status:** Accepted
- **Date:** 2026-10-15

## Context

`environments.toml` decides which targets agents may change without asking. Until now `yuzu check` allowed any command that edited it: `sed -i ... ~/.config/yuzu/environments.toml` came back `allow`, and the hook only looked at Bash. An agent blocked on a host could add the host to the file and run the command, approving itself. This came up while adding a host to the file for the user.

## Decision

- A new target kind, `yuzu_config` (label "yuzu config"), for yuzu's own environments file. No env can declare it, so a write to it always asks and a read is allowed, by the existing rules.
- `check` finds it by text as well as by words, since the shell splitter drops redirections. A simple command that names the file is a write unless its program only reads (`cat`, `less`, `head`, `tail`, `grep`, `rg`, `bat`, `wc`, `diff`, `ls`, `stat`, `file`, `test`). A redirection into it is a write. "Names the file" means the word contains the file's name, its full path, `.config/yuzu`, `CONFIG_HOME/yuzu`, or `YUZU_CONFIG`.
- The hook also handles Claude Code's file tools (`Edit`, `Write`, `MultiEdit`, `NotebookEdit`): it asks when the file is the config and stays silent otherwise. The plugin's matcher widens to `Bash|Edit|Write|MultiEdit|NotebookEdit`.
- A file edit is checked even when the config doesn't parse, so a broken config doesn't make every edit ask.

## Consequences

- An agent can't widen its own permissions without the user seeing the change.
- It errs toward asking: `sed -n p environments.toml`, or any command naming a project file called `environments.toml`, asks too. That's rare enough to accept.
- If yuzu is missing, the hook's fallback now asks on every file edit as well as every Bash command. That's the same fail-safe rule, applied to more tools.
- A shell can still reach the file in ways text can't see, such as a script that builds the path at runtime. The guard catches the ordinary ways, not a determined one.

## Alternatives considered

- **Only guard Bash.** Leaves the file tools open, which is how agents usually edit files.
- **Ask on any mention, reads included.** Agents read the file to explain a decision; asking for that would teach the user to approve without looking.
- **Make the file read-only on disk.** The user would have to undo it to edit, and agents can `chmod` too.
