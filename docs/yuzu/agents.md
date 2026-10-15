# yuzu for agents

How an agent finds and uses yuzu, strongest layer first. Safety never depends on the agent remembering ([../design.md](../design.md#how-agents-find-the-tools)).

## The check hook

`yuzu hook claude-code` is a Claude Code `PreToolUse` hook on Bash and the file tools. It runs `check` on every Bash command before it runs, and asks before `Edit`, `Write`, `MultiEdit`, or `NotebookEdit` changes yuzu's own config ([ADR 0036](../decisions/0036-yuzu-guards-its-config.md)):

- **Allowed:** prints nothing, so Claude Code's own permission rules still apply. yuzu only ever makes things stricter.
- **Ask or deny:** answers with yuzu's message as the reason, so the agent learns what was blocked and what to do instead, and logs it for review ([config.md](config.md#files-yuzu-writes)).
- **Input it can't read:** asks.

The `mochi-donut-hooks` plugin installs it. By hand, the command ends in a fallback, so a missing or crashed binary asks instead of allowing ([ADR 0014](../decisions/0014-claude-code-hook.md)):

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash|Edit|Write|MultiEdit|NotebookEdit",
        "hooks": [
          {
            "type": "command",
            "command": "yuzu hook claude-code || printf '%s' '{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"ask\",\"permissionDecisionReason\":\"yuzu failed or is missing, so this command was not checked.\"}}'",
            "timeout": 10
          }
        ]
      }
    ]
  }
}
```

The user installs it, not the agent: it changes how commands get approved.

## MCP tools

`yuzu mcp` serves these over stdio; the `mochi-donut` plugin configures it.

| Tool | Runs |
|---|---|
| `status` | `yuzu status` |
| `check` | `yuzu check -- <command>` |
| `probe` | `yuzu probe <url>` |
| `whoami` | `yuzu whoami [--verify]` |
| `wait` | `yuzu wait <condition> <target>`, with the timeout defaulting to 5m to stay under client tool-call limits |

The hook isn't a tool: safety checks run as hooks, never on the agent's say-so.

## The skill

`plugins/mochi-donut/skills/yuzu/SKILL.md` loads before work on clusters, accounts, hosts, or remotes, when auth fails, when waiting on something, and when something works from the shell but not for the user. Its rule: when `check` says a command needs approval, ask the user; don't look for another way to run it.

## Trigger lines for a repo's agent instructions

```
When it works for you but not the user            yuzu probe <url>
Before pushing, or when auth fails                yuzu whoami
Instead of sleep or a poll loop                   yuzu wait ci|port|url|file|cmd ...
```

## `yuzu agent-guide`

Prints a compact guide for any agent with a shell. It's `crates/yuzu/AGENT_GUIDE.md`.
