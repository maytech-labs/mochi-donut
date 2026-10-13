---
name: yuzu
description: Use before running commands against Kubernetes clusters, cloud accounts, remote hosts, or git remotes (which one is the shell pointed at, and is it the user's?), and when something works from your shell but the user says it doesn't load for them (probe the browser's side before asking them to debug).
---

# yuzu: what the user's environment is

yuzu knows which clusters, accounts, hosts, and remotes are the user's, which are work or production, and what agents may do to each. Use the MCP tools when they're available (`status`, `check`, `probe`); otherwise run `yuzu <command> --json`. `yuzu agent-guide` prints the full guide.

## When to use what

| Moment | Tool | What you get |
|---|---|---|
| Before acting on a cluster, account, or host | `status` | The current kube context, AWS profile, and git remotes, and whose each is |
| Before a command that might change one | `check` with the command line | What it touches, and whether it's allowed, needs approval, or is denied |
| It works from your shell but not for the user | `probe` with the URL | curl's result and the browser's side by side, and the likely cause with a fix |

## Rules

- When `check` says a command needs the user's approval, ask them. Don't rewrite the command to get around it, and don't switch contexts to make it pass. Use the safe target the message names, if it names one.
- If the user installed the hooks, `check` already runs before every Bash command. You'll see its message when it stops one.
- `probe` reads no cookies, history, or passwords: it uses an empty, temporary browser profile. Run it instead of asking the user to open developer tools, then tell them the cause and the fix it names.
