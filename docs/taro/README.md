# taro

**Proposed, not built.** The forge and the team: whether a PR can merge and what's blocking it, open review feedback, who owns the code, which checks are flaky, and which branches are safe to delete.

| Command | Question it answers |
|---|---|
| `taro ready` | Can this PR merge? If not, what's blocking it? |
| `taro reviews` | What review feedback is still open? |
| `taro owners` | Who owns these files, and who should review? |
| `taro flaky` | Which checks fail intermittently? |
| `taro cleanup` | Which branches are merged and safe to delete? |

## Docs

- [design.md](design.md): the problem, the proposed design, its bars, risks, and open questions
- [usage.md](usage.md): the planned commands

The decision to propose it is [ADR 0028](../decisions/0028-propose-ube-and-taro.md).
