# matcha

What a repo knows, with evidence. matcha answers questions about one repo from its own files, history, and docs, and shows the counts and examples behind every answer.

| Command | Question it answers |
|---|---|
| `census` | How is this usually done here? |
| `recipe` | What else usually changes when I change this? |
| `drift` | Are the docs' claims about the code still true? |
| `ledger` | What did I verify, and what did I assume? |
| `eval` | Are a tool's answers still right? |

It needs no setup: run it inside any git repo, or pass `--repo <path>`. It reads only the repo, writes only `.matcha/` (which ignores itself), and makes no network calls except when an eval spec names a repo by URL.

## Docs

- [design.md](design.md): the problem each command solves, how it works, its bar, and its status
- [usage.md](usage.md): every command and flag, with output and exit codes
- [config.md](config.md): drift's check comments and markers, eval specs, and what's under `.matcha/`
- [agents.md](agents.md): how agents reach it: MCP, the Claude Code skill, the ledger hook, and trigger lines for `AGENTS.md`

Decisions are in [../decisions/](../decisions/); the cross-tool design is in [../design.md](../design.md).
