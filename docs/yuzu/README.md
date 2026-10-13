# yuzu

What your environment is, before you act on it. yuzu knows which clusters, cloud accounts, hosts, and remotes are yours, which are work or production, and what agents may do to each. It works in any directory, repo or not.

| Command | Question it answers |
|---|---|
| `yuzu` (status) | Where does the shell point right now, and whose is it? |
| `check` | What would this command touch, and is it allowed? |
| `probe` | Does this URL work from where the user is, not just from my shell? |
| `whoami` | Which account will gh, git, ssh, and AWS act as here? |
| `wait` | Is CI, the port, the URL, the file, or the command done yet? |
| `sessions` | How have agents used these tools, and what did the hook block? |

Setup is one file, `~/.config/yuzu/environments.toml`, declaring whose environments are whose: see [config.md](config.md). Its safety value comes from running `check` as a hook before every command an agent runs.

## Docs

- [design.md](design.md): the problem each command solves, how it works, its bar, and its status
- [usage.md](usage.md): every command and flag, with output and exit codes
- [config.md](config.md): `~/.config/yuzu/environments.toml`, and the environment variables yuzu reads
- [agents.md](agents.md): the check hook, MCP tools, the Claude Code skill, and trigger lines

Decisions are in [../decisions/](../decisions/); the cross-tool design is in [../design.md](../design.md).
