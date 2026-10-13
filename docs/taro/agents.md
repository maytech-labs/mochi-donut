# taro for agents

## MCP tools

`taro mcp` serves these over stdio; the `mochi-donut` plugin configures it.

| Tool | Runs |
|---|---|
| `ready` | `taro ready [pr]` |
| `reviews` | `taro reviews [pr]` |
| `owners` | `taro owners [paths]`, or `--diff` without paths |
| `flaky` | `taro flaky [--since]` |

`cleanup` isn't a tool: deleting branches is the user's call.

## No hooks

Everything taro does is a network call, so nothing runs it on every command.

## The skill

`plugins/mochi-donut/skills/taro/SKILL.md` loads before asking to merge, when addressing reviews, when choosing reviewers, before debugging a CI failure, and after merging. Its rule: report, don't act. Don't merge or delete unless the user asks.

## Trigger lines for a repo's agent instructions

```
Before asking to merge                            taro ready
Before debugging a CI failure                     taro flaky
```

## `taro agent-guide`

Prints a compact guide for any agent with a shell. It's `crates/taro/AGENT_GUIDE.md`.
