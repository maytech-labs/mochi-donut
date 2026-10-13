# mochi-donut

Tools that help AI coding agents work in a repo the way a careful teammate would: with evidence about how things are done here, and without touching the wrong environment.

> **Status: pre-release.** Every command works: `matcha census`, `recipe`, `drift`, `ledger`, and `eval`, and `yuzu` status, `check`, `probe`, `whoami`, `wait`, and `sessions`; the rest of the commands parse and say they aren't built yet. See the [design](docs/design.md) and the [roadmap](#roadmap).

Agents start every session with no memory. They guess the house style, forget the companion edits a change needs, trust docs that have drifted, and run commands against whatever cluster or account the shell happens to point at. mochi-donut answers those questions from the repo's own history and from a declaration of your environments, in a form agents can act on.

Like a pon de ring, it's a ring of separate pieces. Each tool is named after a mochi donut flavor.

| Tool | Knows about | Commands |
|---|---|---|
| **matcha** | The repo: its files, history, and docs | `census` (how is this usually done here?), `recipe` (what else changes with this?), `drift` (are the docs still true?), `ledger` (what did I verify, and what did I assume?), `eval` (are the answers still right?) |
| **yuzu** | Your environment: clusters, accounts, hosts, browser | `status` (where am I?), `check` (what will this command touch, and is it allowed?), `probe` (does it work from where the user is?), `whoami` (which account will this act as?), `wait` (is it done yet?), `sessions` (how do agents use these tools?) |

Every command has `--json` output and stable exit codes, and each tool prints a guide written for agents:

```sh
matcha agent-guide
yuzu agent-guide
```

## Install

With Homebrew (builds the latest `main` until the first release):

```sh
brew tap trammel-may/mochi-donut https://github.com/trammel-may/mochi-donut
brew install --HEAD mochi-donut
brew upgrade --fetch-HEAD mochi-donut   # later, to update
```

From a clone, into `/usr/local/bin` (or set `PREFIX`):

```sh
make install
make install PREFIX=~/.local   # somewhere else
make uninstall
```

With Cargo:

```sh
cargo install --git https://github.com/trammel-may/mochi-donut mochi-matcha mochi-yuzu
```

Pick one way; two installs means two copies on your `PATH`. The packages are named `mochi-matcha` and `mochi-yuzu`; the commands they install are `matcha` and `yuzu`.

Note: Homebrew has an unrelated `matcha` (an RSS digest tool). The formula conflicts with it, and if you install another way, whichever comes first on your `PATH` wins.

## Set up yuzu

Declare whose environments are whose in `~/.config/yuzu/environments.toml`:

```toml
[[env]]
name = "homelab"
class = "mine"                    # mine, work, or production
kube_contexts = ["k3s-pis"]       # patterns: * and ?
ssh_hosts = ["pi-1", "pi-2"]
git_remotes = ["github.com/me/*"]

[[env]]
name = "work"
class = "work"                    # agents can read; writes ask you first
kube_contexts = ["*-nonprod-*"]
```

Run `yuzu` to see where your shell points, and `yuzu check -- '<command>'` to see what a command would touch. To check every command Claude Code runs, add the hook in [ADR 0014](docs/decisions/0014-claude-code-hook.md) to `~/.claude/settings.json`.

## Use with agents

In Claude Code, this repo is a plugin marketplace with two plugins:

```sh
/plugin marketplace add trammel-may/mochi-donut
/plugin install mochi-donut@mochi-donut         # MCP servers and skills for both tools
/plugin install mochi-donut-hooks@mochi-donut   # yuzu checks every Bash command; matcha records evidence
```

Both need `matcha` and `yuzu` on your `PATH`. The hooks plugin is separate because it changes how commands get approved: install it on purpose. You can add the same hooks by hand instead ([ADR 0014](docs/decisions/0014-claude-code-hook.md), [ADR 0017](docs/decisions/0017-ledger.md)).

Other agents with MCP can run `matcha mcp` and `yuzu mcp` as stdio servers. Any agent can learn either tool from `matcha agent-guide` or `yuzu agent-guide`.

For a repo's own agent instructions (`AGENTS.md`, `CLAUDE.md`), a short trigger table works better than a manual:

```
Before copying a pattern or writing a guideline   matcha census
Before a change you haven't made here before      matcha recipe --like "<what you're doing>"
Before trusting a doc                             matcha drift
When you check or assume a fact                   matcha ledger add verified|assumed "<claim>"
Before committing                                 matcha recipe --check; matcha ledger check --diff
When it works for you but not the user            yuzu probe <url>
```

## Roadmap

1. `matcha census` and `matcha recipe` (built; `recipe` clears its 90% precision bar on Ghostty and vim but not on Anchor, see [ADR 0012](docs/decisions/0012-recipe-support-six.md))
2. `yuzu status` and `yuzu check`, run as a pre-command hook (built; on a two-week trial, see [ADR 0014](docs/decisions/0014-claude-code-hook.md))
3. `matcha drift` (mechanical claims built and meets its bar on Anchor, and check comments built, see ADRs [0015](docs/decisions/0015-drift-mechanical-claims.md) and [0016](docs/decisions/0016-drift-check-comments.md)) and `matcha ledger` (built, see [ADR 0017](docs/decisions/0017-ledger.md))
4. `yuzu probe` (built, see [ADR 0018](docs/decisions/0018-yuzu-probe.md))
5. MCP servers and a Claude Code plugin with skills and hooks (built, see [ADR 0019](docs/decisions/0019-mcp-and-plugin.md))
6. `yuzu whoami`, `yuzu wait`, `yuzu sessions`, and `matcha eval`, from friction in real sessions (built, see ADRs [0022](docs/decisions/0022-yuzu-whoami.md) to [0025](docs/decisions/0025-yuzu-sessions.md))

Nothing is announced or published to crates.io until the first two steps prove useful ([ADR 0004](docs/decisions/0004-prove-before-announcing.md)).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). AI agents working in this repo start with [AGENTS.md](AGENTS.md).

## License

[MIT](LICENSE)
