# 0022. yuzu whoami: which account each tool acts as, offline unless asked

- **Status:** Accepted
- **Date:** 2026-10-13

## Context

Opening a PR failed twice in one session: `GH_TOKEN` held a token GitHub rejected, and it silently overrode the working `gh` login for github.com. Nothing showed which credentials `gh` would use, or that one hid another. The same kind of thing happens with AWS keys in the environment and a profile, and with which key ssh offers a host. yuzu already answers "whose cluster is this?"; this answers "whose account is this?".

## Decision

- **`yuzu whoami`** lists, for the current directory:
  - **github**: each host in gh's `hosts.yml` (`$GH_CONFIG_DIR`, `$XDG_CONFIG_HOME/gh`, or `~/.config/gh`) and its active user, and whether `GH_TOKEN`/`GITHUB_TOKEN` (github.com, `*.ghe.com`) or `GH_ENTERPRISE_TOKEN`/`GITHUB_ENTERPRISE_TOKEN` (other hosts) overrides it. An override is a warning that says what it hides.
  - **git**: the commit author and the file each value comes from (`git config --show-origin`), and whether commits are signed.
  - **ssh**: for each ssh remote, the key files `ssh -G` names that exist.
  - **aws**: the profile, or static keys in `AWS_ACCESS_KEY_ID`, with a warning when keys hide a profile.
- **Offline by default.** It reads config files and runs `git config` and `ssh -G`, which don't connect. AGENTS.md rule 5 allows network calls only where a command is for them.
- **`--verify` asks each service**: `gh api user --hostname`, `ssh -T` (reading GitHub's "Hi ...!", GitLab's "Welcome to GitLab, @...", and "logged in as ..."), and `aws sts get-caller-identity`. A failure says what to do: unset the overriding variable, log in again, or refresh the profile. Exit 1 when any check fails.
- **An MCP tool** (`whoami`) for agents, described as "use before pushing, opening a PR, or calling a cloud API, and whenever auth fails".
- The process runner probe used moved to `proc.rs`, shared by probe, whoami, and wait.

## Results (2026-10-13)

On this Mac, offline: it flags `GH_TOKEN` hiding the github.com login, shows commits go out as the noreply address from `~/.dotfiles/git/.gitconfig-personal`, and names `~/.ssh/id_rsa` for the remote. With `--verify` (5 s): the Enterprise login and the ssh key work; `GH_TOKEN` gets HTTP 401; the default AWS profile's token is invalid.

## Consequences

- An agent can tell a credentials problem from a code problem in one call, and the user gets the exact variable to unset.
- Kube users, GCP and Azure accounts, and npm or cargo registry tokens aren't covered yet.

## Alternatives considered

- **Always verify:** the most useful answer, but network calls on every run, against rule 5, and a 5-second command.
- **Fold it into `yuzu status`:** status runs before risky commands and must stay fast and local; identity checks with `--verify` don't fit there.
