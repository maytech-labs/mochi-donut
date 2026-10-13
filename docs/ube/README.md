# ube

**Proposed, not built.** The change in hand: which checks apply to it, whether they've run on exactly this code, what failed in a few lines, and whether its tests can fail at all.

| Command | Question it answers |
|---|---|
| `ube plan` | Which checks apply to this change? |
| `ube run` | Run them: what failed, in a few lines? |
| `ube done` | Is this change done, with evidence? |
| `ube mutate` | Can the tests for this change fail at all? |

## Docs

- [design.md](design.md): the problem, the proposed design, its bars, risks, and open questions
- [usage.md](usage.md): the planned commands

The decision to propose it is [ADR 0028](../decisions/0028-propose-ube-and-taro.md).
