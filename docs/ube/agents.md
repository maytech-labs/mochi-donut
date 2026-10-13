# ube for agents

## The Stop hook

`ube hook claude-code` is a Claude Code `Stop` hook. When the agent ends its turn with a change whose planned checks haven't passed on the current code, it blocks the stop once, with the checks to run. It stays silent when there are no changes, when there are no checks, when everything has passed, and when the agent is already continuing because of it, so it can't loop.

It isn't in the `mochi-donut-hooks` plugin yet: whether it helps or nags is what a trial decides. To try it, in Claude Code's settings:

```json
{
  "hooks": {
    "Stop": [
      { "hooks": [{ "type": "command", "command": "ube hook claude-code", "timeout": 60 }] }
    ]
  }
}
```

## MCP tools

`ube mcp` serves these over stdio; the `mochi-donut` plugin configures it.

| Tool | Runs |
|---|---|
| `plan` | `ube plan [--base] [--all]` |
| `run` | `ube run [--base] [--all] [--rerun-failed] [-- <command>]` |
| `done` | `ube done [--base]` |

`mutate` isn't a tool: it can take minutes, longer than a tool call should.

## The skill

`plugins/mochi-donut/skills/ube/SKILL.md` loads when testing a change or before saying it's done. Its rule: don't say a change works until `done` passes, or say plainly what it reports as failing or unknown.

## Trigger lines for a repo's agent instructions

```
To test a change                                  ube run
Before saying a change is done                    ube done
```

## `ube agent-guide`

Prints a compact guide for any agent with a shell. It's `crates/ube/AGENT_GUIDE.md`.
