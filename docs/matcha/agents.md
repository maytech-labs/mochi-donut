# matcha for agents

How an agent finds and uses matcha, strongest layer first ([../design.md](../design.md#how-agents-find-the-tools)).

## The ledger hook

`matcha hook claude-code` is a Claude Code `PostToolUse` hook on Bash. It records each command and an excerpt of its output, so `matcha ledger add verified --cmd <text>` can attach it as evidence. It prints nothing, always exits 0, and takes about 17 ms. The `mochi-donut-hooks` plugin installs it; by hand, in Claude Code's settings:

```json
{
  "hooks": {
    "PostToolUse": [
      { "matcher": "Bash", "hooks": [{ "type": "command", "command": "matcha hook claude-code", "timeout": 10 }] }
    ]
  }
}
```

## MCP tools

`matcha mcp` serves these over stdio; the `mochi-donut` plugin configures it. Each runs the CLI with `--json` and returns what it printed. Exit 1 (a check found something) is an answer, not an error.

| Tool | Runs |
|---|---|
| `census` | `matcha census` with `alts`, `spelling`, `in`, `examples`, `label` |
| `recipe_like` | `matcha recipe --like <text>` |
| `recipe_check` | `matcha recipe --check [--commit]` |
| `drift` | `matcha drift [--in] [--suggest]` |
| `ledger_add`, `ledger_verify`, `ledger_list`, `ledger_check`, `ledger_summary` | The `ledger` commands |

Every tool takes an optional `repo`. Descriptions lead with the question they answer ("Use when asked how this repo usually does something ..."); in a headless test, that wording is what made an agent call `census` instead of grepping ([ADR 0021](../decisions/0021-usability-from-trying-the-tools.md)).

## The skill

`plugins/mochi-donut/skills/matcha/SKILL.md` loads when a task matches its description: how the repo usually does something, a change not made here before, trusting a doc, or a fact to record. It holds the "when to use what" table and how to read results.

## Trigger lines for a repo's agent instructions

```
Before copying a pattern or writing a guideline   matcha census
Before a change you haven't made here before      matcha recipe --like "<what you're doing>"
Before trusting a doc                             matcha drift
When you check or assume a fact                   matcha ledger add verified|assumed "<claim>"
Before committing                                 matcha recipe --check; matcha ledger check --diff
```

## `matcha agent-guide`

Prints a compact guide for any agent with a shell. It's `crates/matcha/AGENT_GUIDE.md`.
