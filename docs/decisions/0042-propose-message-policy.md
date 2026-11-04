# 0042. Propose a declared policy for commit and PR messages

- **Status:** Proposed
- **Date:** 2026-11-04

## Context

[ADR 0006](0006-commit-messages.md) sets this repo's message rules: Conventional Commits, about 50 characters, no AI attribution. Nothing enforces them. An agent follows them only if it read `AGENTS.md` and overrode its tool's defaults, and the owner wants the same rules in every repo, not just this one. Agents that forget are the case the design says hooks are for: safety and hygiene never depend on the agent remembering ([design](../design.md#how-agents-find-the-tools)).

`taro ready` already learns some conventions from merged PRs (subject form, changelog entries, ADRs) and checks a PR against them ([ADR 0030](0030-taro.md)). It runs only at the end, needs 10 merged PRs, and knows nothing about rules the owner declares but history can't show, such as no attribution in a new repo.

Rule 3 in `AGENTS.md` is "evidence, not opinion". A built-in "never mention Claude" rule is an opinion, and a wrong one here: this repo ships a Claude Code plugin, so `feat: claude code hook` is a legitimate subject.

## Decision

- **`ube message`** checks a commit or PR message against the policy: `ube message --file <path>`, or stdin. It belongs in ube because the message is part of the change in hand. Exit 0 when the message is clean, 1 with each problem and the fix, like every check (`--json`, `mochi_common::exit`).
- **The policy is declared, not built in.** A user file, `~/.config/ube/messages.toml`, holds the owner's rules for every repo. A repo's `.ube.toml` can add rules but not remove the user's. With no file, `ube message` checks only what history shows.
  ```toml
  forbid_trailers = ["Co-Authored-By: *Claude*", "Co-Authored-By: *noreply@anthropic.com*"]
  forbid_lines = ["*Generated with*Claude*", "🤖*"]
  subject_max = 50
  subject_form = "conventional"   # or "learned": whatever history shows
  ```
  Patterns match trailers and lines, not words anywhere in the message, so a subject that names a tool passes.
- **Learned conventions come from history**, measured the way `taro ready` measures them, with the counts shown. The code that learns them moves from taro into `mochi-index` so both tools share it.
- **Three places run it:**
  1. A git `commit-msg` hook (`ube message --file "$1"`), which sees the final message whatever agent or person wrote it. `ube hook git --install` writes it.
  2. The `mochi-donut-hooks` plugin, before `gh pr create` and `gh pr edit` with a body, since git has no hook for PRs.
  3. `taro ready`, over every commit on the branch and the PR body, as the last check before merging.
- **Report-only first.** It warns until a trial measures its false-alarm rate, then blocks, as the design's risks section requires for new checks.
- **ADR scaffolding isn't part of this.** Making every repo keep `docs/decisions/` produces content, which the design rules out. That's a job for a skill in the plugin, proposed separately. Flagging "PRs here usually add an ADR and this one doesn't" stays with `taro ready`.

## Consequences

- The owner's rules follow them into every repo, for any agent, without the agent having read anything.
- ube gets its first config file. Its "no configuration" promise in [config.md](../ube/config.md) becomes "configuration is optional".
- `git commit --no-verify` skips the git hook. `taro ready` still catches the message before merge, and the plugin could flag `--no-verify` later if trials show agents use it.
- Matching is by pattern, so a trailer in a form no pattern names gets through. The user adds the pattern; the output of a miss can't teach that, but the defaults in the usage docs can.

## Alternatives considered

- **Built-in rules for AI attribution:** no setup, but it's opinion, and some owners want those trailers.
- **Only Claude Code's `attribution` setting:** stops the trailer at the source for one agent, and is worth setting anyway, but doesn't cover other agents, footers, or subject form.
- **A plain `commit-msg` script per repo:** quick, but a copy in every repo, no learned conventions, and nothing for PRs.
- **Put it in yuzu:** yuzu already holds a per-user declaration and runs before every command, but its check must stay small, and parsing messages out of shell commands (heredocs, `-F` files) is the job the git hook does better.
- **Put it only in `taro ready`:** one place, but it finds the problem at the end, after the commits are written and pushed.
