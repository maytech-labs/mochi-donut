# 0013. How yuzu check finds targets and decides

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

The design says what `yuzu check` is for and its bar: with the homelab declared, a write against any other context is stopped before it runs, and reads still work. It leaves open which commands to understand, how to read a shell line, what "stopped" means, and the config's exact shape.

## Decision

- **Programs covered first:** `kubectl` and `helm` (kube context), `ssh`, `scp`, `sftp`, `rsync` (host), `git push` (remote URL), and `aws` (profile). These are what reach shared systems from this Mac today. Not yet: `docker` contexts, `gcloud`, `az`, `terraform`, `gh`, and `kubectl --server`.
- **Reading a shell line:** a small splitter in `shell.rs`, not a shell parser crate. It finds every simple command across `;`, `&&`, `||`, `|`, newlines, subshells, `{ }`, `$( )`, and backticks, removes quotes, drops redirections, and skips heredoc bodies. It looks through `sudo`, `env`, `timeout`, `watch`, `xargs`, `bash -c`, `eval`, and `if`/`for` keywords, and follows `VAR=value`, `export`, and `cd` within the line. It doesn't expand variables: `ssh $h` checks a host named `$h`, which is undeclared, so it asks.
- **Read or write:** by subcommand. kubectl `get`, `describe`, `logs`, `diff`, and similar, plus anything with `--dry-run`, are reads. `exec`, `port-forward`, and `cp` are writes, since they give a way in. An unknown subcommand is a write. Anything over `ssh` is a write.
- **Decisions:** an environment's `agents` is `write`, `read-only`, or `none`, defaulting to `write` for `mine` and `read-only` for `work` and `production`.
  - Writes to a `read-only` environment, or to anything undeclared, **ask the user**. They aren't denied outright, because the design wants writes outside `mine` to need explicit approval, and asking is that approval.
  - `none` denies, reads too.
  - The strictest target in a line decides.
- **Kube context:** `--context`, else the first `current-context` across `$KUBECONFIG` (or `--kubeconfig`), else `~/.kube/config`, read with a line scan instead of a YAML crate. No context at all means kubectl can't reach anything, so there's no target. A kubeconfig that can't be read is an `(unknown)` target.
- **Git remotes** are normalized to `host/owner/repo`, lowercased, so one pattern matches SSH and HTTPS spellings. A bare `git push` follows git's own defaults (`pushRemote`, `pushDefault`, the upstream, `origin`), then the only remote if there's just one.
- **Config:** TOML at `$YUZU_CONFIG`, else `$XDG_CONFIG_HOME/yuzu/environments.toml`, else `~/.config/yuzu/environments.toml`. Plural list keys (`kube_contexts = [...]`), not the design sketch's single strings, so one environment can hold several. Unknown keys are errors. A config that doesn't parse makes every check ask, so a typo can't quietly allow everything. The first matching environment wins.
- **Dependency:** `toml` with only its parser (`default-features = false`). yuzu grew from 506 KB to 834 KB.

## Results (2026-10-12)

Replayed all 2,118 Bash commands from this Mac's Claude Code sessions through the hook, with this Mac's declarations. One asked: a loop that ran `ssh $h` over four hosts, one of them undeclared. Every homelab write was allowed, and so was every read of the work cluster. No wrong blocks. Each check takes about 10 ms.

## Consequences

- yuzu can be wrong about commands it doesn't understand: a tool it doesn't cover passes, and a variable it can't expand asks. The covered list grows when a real session needs it.
- Hosts and remotes have to be declared before agents can write to them without asking. That's the point, but it costs a setup step.

## Alternatives considered

- **Deny writes outside `mine`:** simpler, but the user then has to run the command themselves even when they'd approve it.
- **A real shell parser crate** (such as `conch-parser`): unmaintained or large, and still wouldn't expand variables. The splitter covers what agents write.
- **Classify ssh remote commands by what they run:** tempting for `ssh host 'kubectl get pods'`, but a host is someone's whole machine; its class decides.
