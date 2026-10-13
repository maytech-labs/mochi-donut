# taro

The forge and the team: whether a PR can merge and what's blocking it, open review feedback, who owns the code, which checks are flaky, and which branches are proved merged.

| Command | Question it answers |
|---|---|
| `ready` | Can this PR merge? If not, what's blocking it? |
| `reviews` | What review feedback is still open? |
| `owners` | Who owns these files, and who should review? |
| `flaky` | Which checks fail intermittently? |
| `cleanup` | Which branches are merged and safe to delete? |

`ready`, `reviews`, and `flaky` talk to GitHub or GitHub Enterprise through `gh`; `owners` and `cleanup` use only the local clone. GitLab isn't supported yet.

## Docs

- [design.md](design.md): the problem, the design, its bars, and status
- [usage.md](usage.md): every command and flag, with output and exit codes
- [agents.md](agents.md): MCP tools, the skill, and trigger lines

Decisions: [ADR 0028](../decisions/0028-propose-ube-and-taro.md) proposed it; [ADR 0030](../decisions/0030-taro.md) records how it was built.
