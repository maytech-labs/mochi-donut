# ube

The change in hand: which of the repo's checks apply to it, whether they passed on exactly this code, what failed in a few lines, and whether its tests can fail at all.

| Command | Question it answers |
|---|---|
| `plan` | Which checks apply to this change? |
| `run` | Run them: what failed, in a few lines? |
| `done` | Is this change done, with evidence? |
| `mutate` | Can the tests for this change fail at all? |

It runs only checks the repo already declares (Cargo packages, npm scripts, Go packages, pytest, the Makefile, CI), writes results to `.ube/` (which ignores itself), and makes no network calls of its own.

## Docs

- [design.md](design.md): the problem, the design, its bars, and status
- [usage.md](usage.md): every command and flag, with output and exit codes
- [config.md](config.md): how checks are found, and what's under `.ube/`
- [agents.md](agents.md): the Stop hook, MCP tools, the skill, and trigger lines

Decisions: [ADR 0028](../decisions/0028-propose-ube-and-taro.md) proposed it; [ADR 0029](../decisions/0029-ube.md) records how it was built.
