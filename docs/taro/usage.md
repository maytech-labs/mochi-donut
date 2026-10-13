# Using taro

Every command takes `--json` (one JSON object on stdout) and `--repo <host/owner/repo>` (`owner/repo` means github.com; default: the push remote's). Exit codes:

| Code | Meaning |
|---|---|
| 0 | Nothing wrong |
| 1 | Not ready to merge, open review threads, or a delete failed |
| 2 | Bad usage, no repo, or the forge couldn't be read (with what to do) |

`taro agent-guide` prints a short version of this page, written for agents.

## ready

```sh
taro ready          # the current branch's PR
taro ready 42
```

Each item is `ok`, `warn`, `BLOCK`, or unknown, with what to do:

| Item | Blocks when |
|---|---|
| `state`, `draft` | The PR is closed, merged, or a draft |
| `conflicts` | It conflicts with its base |
| `behind` | (warns) The base has moved on |
| `checks` | A required check failed, is still running, or wasn't reported. If the required checks can't be read, every check counts, and it says so. |
| `reviews` | Changes were requested, or an approval is required |
| `threads` | A review thread is unresolved |
| `changelog`, `adr`, `title` | At least 90% of the last 30 merged PRs (and at least 10 of them) changed a changelog, an ADR, or had a Conventional Commits title, and this one doesn't. From 60%, it warns. |

## reviews

```sh
taro reviews 42
taro reviews 42 --all     # resolved ones too
```

Each thread: file and line, who opened it, the latest comment and its author, its link, and whether the code changed since. Exit 1 while any is unresolved.

## owners

```sh
taro owners --diff                  # the change against the default branch
taro owners crates/yuzu/ docs/      # paths
```

| Flag | |
|---|---|
| `--diff` | The files this branch changes against `--base`, plus uncommitted ones (the default without paths) |
| `--base <REV>` | Default: the remote's default branch |
| `--since <AGE>` | History to count authors in: `1.year` (default), `6.months` |

Owners come from `CODEOWNERS` (in `.github/`, the root, or `docs/`; the last matching rule wins). Then the authors who changed those paths most, and suggested reviewers without you. Local only.

## flaky

```sh
taro flaky
taro flaky --since 14d
```

From GitHub Actions runs in the window: a run that passed on a rerun after a failed attempt (named by job), or a workflow that both failed and passed on one commit. Each with how often, out of how many runs, and its latest example. A check that only ever fails for real isn't listed.

## cleanup

```sh
taro cleanup                         # local branches against the default branch
taro cleanup --remote                # the push remote's branches too
taro cleanup --delete                # delete the proved ones, after asking
taro cleanup --delete --yes          # without asking
```

A branch is merged when its content is in the base, proved by ancestry (merge commits), by every commit's patch being there (rebase merges), by one commit equal to the whole branch (squash merges), or by every file it changed matching. Anything else is kept, with the number of commits not in the base and one of their subjects. `--delete` asks first; with no terminal to ask on, it needs `--yes`. Remote branches are deleted with `git push <remote> --delete`, which yuzu's hook checks like any push.

## Serving agents

`taro mcp` serves `ready`, `reviews`, `owners`, and `flaky` as MCP tools; see [agents.md](agents.md).
