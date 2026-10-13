# 0023. yuzu wait: one command for waiting on CI, ports, URLs, files, and commands

- **Status:** Accepted
- **Date:** 2026-10-13

## Context

Waiting for CI took four tries in one session: `gh pr checks --watch` and `gh run watch` returned before the run finished, and two hand-written poll loops ended early. Agents wait constantly, and every hand-rolled loop is a new chance to get the timeout, the exit code, or the end condition wrong.

## Decision

- **`yuzu wait <condition>`** with `ci [PR|ref]`, `port host:port`, `url URL [--status N]`, `file PATH`, and `cmd -- COMMAND` (one argument runs through `sh -c`). `--not` waits until it stops being true.
- **Flags on every condition:** `--timeout` (default 10m), `--every` (default 5s, 10s for url, 30s for ci), written `90s`, `10m`, `1h`. The last look happens at the deadline, not one interval before it.
- **Exit 0 when it holds, 1 when it times out or settles as failed** (CI failed), 2 for bad usage. `--json` gives the condition, met, timed_out, elapsed, looks, and the last observation with failed checks and their links.
- **Progress goes to stderr, and only when what's seen changes**, so an agent's context doesn't fill with identical lines.
- **CI through `gh`**: a number is a PR (`gh pr checks --json name,bucket,link`); anything else is resolved to a full commit and its workflow runs are listed (`gh run list --commit`; short SHAs match nothing). The repo comes from `--repo host/owner/repo`, else the branch's remote, else `origin`, else the first remote. "No checks" waits two minutes, then fails as "is CI set up for this?". An authentication error fails at once and points at `yuzu whoami --verify`: waiting won't fix credentials.
- **GitLab isn't supported yet**, and says so with the `wait cmd` alternative, rather than guessing at `glab`'s flags.
- **In yuzu**, since what it waits on is the environment. Also an MCP tool, whose timeout defaults to 5 minutes to stay under client tool-call limits.

## Results (2026-10-13)

`wait ci 2` returned "all 2 passed" in a second; `wait ci upstream/main` found the main push's run; `wait port` caught a server started 6 seconds in; a bad `GH_TOKEN` failed at once with the pointer to `whoami`.

## Consequences

- Agents get one tool for every wait, with the same flags, exit codes, and JSON.
- CI providers other than GitHub need their own adapters; `wait cmd` covers them meanwhile.

## Alternatives considered

- **A third tool:** waiting is about the environment, which is yuzu's; a third binary goes against ADR 0003's two-tool split.
- **`kubectl wait`, `gh run watch`, and friends:** each covers one thing, with its own flags and exit codes, and `gh run watch` is what returned early.
