# 0014. yuzu runs as a Claude Code hook that's silent when it allows

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

The design wants `yuzu check` to run before every shell command whether or not the agent remembers. Claude Code's `PreToolUse` hooks get the command as JSON on stdin and can answer `allow`, `ask`, or `deny`. A hook that crashes or exits with a code other than 0 or 2 doesn't stop the command.

## Decision

- **`yuzu hook claude-code`** reads the hook input and checks Bash commands, using the input's `cwd`. Other tools pass untouched.
- **Allowed commands get no output**, not `allow`. An `allow` answer would skip Claude Code's own permission rules. yuzu only ever makes things stricter.
- **`ask` and `deny` answers carry yuzu's message** as the reason, so the agent learns what was blocked and what to do instead.
- **The hook always exits 0**, with the decision in the JSON. Input it can't read gets `ask`.
- **The settings entry ends in a fallback**, so a missing or crashed binary asks instead of allowing:

  ```json
  {
    "hooks": {
      "PreToolUse": [
        {
          "matcher": "Bash",
          "hooks": [
            {
              "type": "command",
              "command": "/usr/local/bin/yuzu hook claude-code || printf '%s' '{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"ask\",\"permissionDecisionReason\":\"yuzu failed or is missing, so this command was not checked.\"}}'",
              "timeout": 10
            }
          ]
        }
      ]
    }
  }
  ```

- **The user installs it.** It changes how Claude Code grants permissions, so an agent shouldn't add it to its own settings.

## Consequences

- ADR 0004's two-week trial starts when the hook is installed.
- Other agents need their own `yuzu hook <agent>` adapters. The check itself doesn't change.

## Alternatives considered

- **A `yuzu check` wrapper the agent calls:** the agent has to remember, which the design rules out for safety checks.
- **Exit code 2 to block:** that denies outright and can't ask the user.
