# mochi-donut

Tools that help AI coding agents work in a repo the way a careful teammate would: with evidence about how things are done here, and without touching the wrong environment.

> **Status: pre-release.** All four tools are built and tested. Nothing is published yet. Nothing is published yet; see the [roadmap](#roadmap) and the [design](docs/design.md).

Agents start every session with no memory. They guess the house style, forget the companion edits a change needs, trust docs that have drifted, and run commands against whatever cluster or account the shell happens to point at. mochi-donut answers those questions from the repo's own history and from a declaration of your environments, in a form agents can act on.

Like a pon de ring, it's a ring of separate pieces. Each tool is named after a mochi donut flavor, and each has its own docs: design, usage, configuration, and how agents use it ([docs/](docs/README.md)).

| Tool | Knows about | Commands |
|---|---|---|
| **[matcha](docs/matcha/README.md)** | The repo: its files, history, and docs | `census` (how is this usually done here?), `recipe` (what else changes with this?), `drift` (are the docs still true?), `ledger` (what did I verify, and what did I assume?), `eval` (are the answers still right?) |
| **[yuzu](docs/yuzu/README.md)** | Your environment: clusters, accounts, hosts, browser | `status` (where am I?), `check` (what will this command touch, and is it allowed?), `probe` (does it work from where the user is?), `whoami` (which account will this act as?), `wait` (is it done yet?), `sessions` (how do agents use these tools?) |
| **[ube](docs/ube/README.md)** | The change in hand: which checks apply, and whether they passed on this code | `plan`, `run`, `done`, `mutate` |
| **[taro](docs/taro/README.md)** | The forge and the team: reviews, checks, owners, branches | `ready`, `reviews`, `owners`, `flaky`, `cleanup` |

Every command has `--json` output and stable exit codes, and each tool prints a guide written for agents:

```sh
matcha agent-guide
yuzu agent-guide
```

## Install

With Homebrew (builds the latest `main` until the first release):

```sh
brew tap maytech-labs/mochi-donut https://github.com/maytech-labs/mochi-donut
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
cargo install --git https://github.com/maytech-labs/mochi-donut mochi-matcha mochi-yuzu mochi-ube mochi-taro
```

Pick one way; two installs means two copies on your `PATH`. The packages are named `mochi-matcha`, `mochi-yuzu`, `mochi-ube`, and `mochi-taro`; the commands they install are `matcha`, `yuzu`, `ube`, and `taro`.

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
aws_profiles = ["personal"]
infra_dirs = ["~/homelab/infra/*"] # where terraform, tofu, or pulumi run
cli_tools = ["flyctl"]            # cloud CLIs whose account is yours

[[env]]
name = "work"
class = "work"                    # agents can read; writes ask you first
kube_contexts = ["*-nonprod-*"]
# agents = "none"                 # or forbid agents entirely
```

Run `yuzu` to see where your shell points, `yuzu check -- '<command>'` to see what a command would touch, and `yuzu whoami` to see which accounts gh, git, ssh, and AWS will act as. To check every command Claude Code runs, install the hooks plugin (below) or add the hook in [ADR 0014](docs/decisions/0014-claude-code-hook.md) to your settings.

## Use with agents

In Claude Code, this repo is a plugin marketplace with two plugins:

```sh
/plugin marketplace add maytech-labs/mochi-donut   # or the path to a clone
/plugin install mochi-donut@mochi-donut         # MCP servers and skills for both tools
/plugin install mochi-donut-hooks@mochi-donut   # yuzu checks every Bash command; matcha records evidence
```

Both need the binaries on your `PATH`. The hooks plugin is separate because it changes how commands get approved: install it on purpose. You can add the same hooks by hand instead ([ADR 0014](docs/decisions/0014-claude-code-hook.md), [ADR 0017](docs/decisions/0017-ledger.md)).

Other agents with MCP can run `matcha mcp` and `yuzu mcp` as stdio servers. Any agent can learn either tool from `matcha agent-guide` or `yuzu agent-guide`.

For a repo's own agent instructions (`AGENTS.md`, `CLAUDE.md`), a short trigger table works better than a manual:

```
Before copying a pattern or writing a guideline   matcha census
Before a change you haven't made here before      matcha recipe --like "<what you're doing>"
Before trusting a doc                             matcha drift
When you check or assume a fact                   matcha ledger add verified|assumed "<claim>"
Before committing                                 matcha recipe --check; matcha ledger check --diff
When it works for you but not the user            yuzu probe <url>
To test a change, and before saying it's done       ube run; ube done
Before asking to merge                            taro ready
Before debugging a CI failure                     taro flaky
Before pushing, or when auth fails                yuzu whoami
Instead of sleep or a poll loop                   yuzu wait ci|port|url|file|cmd ...
```

## What stays on your machine

Nothing leaves it: no telemetry, and the only network calls are the ones a command is for: `yuzu probe`, `yuzu whoami --verify`, `yuzu wait` (CI, a port, a URL), `taro ready`, `reviews`, and `flaky` (GitHub, through `gh`), and `matcha eval` when a spec names a repo by URL. The tools write only these files, all local:

| File | Written by | Holds |
|---|---|---|
| `.matcha/` in a repo | `matcha ledger` and its hook | Claims, and excerpts of command output with secret-looking values redacted. It ignores itself, so it's never committed. |
| `.ube/` in a repo | `ube run` | Each check's result, keyed by the exact tree it ran on. It ignores itself. |
| `~/.local/state/yuzu/decisions.jsonl` | yuzu's hook | Each command it asked about or denied, for review. `YUZU_NO_LOG=1` turns it off. |

`yuzu sessions` reads Claude Code's own transcripts in `~/.claude*/projects`, and writes nothing.

## Roadmap

1. `matcha census` and `matcha recipe` (built; `recipe` clears its 90% precision bar on Ghostty and vim but not on Anchor, see [ADR 0012](docs/decisions/0012-recipe-support-six.md))
2. `yuzu status` and `yuzu check`, run as a pre-command hook (built; the two-week trial starts when the hook is installed, and `yuzu sessions` measures it, see ADRs [0014](docs/decisions/0014-claude-code-hook.md) and [0025](docs/decisions/0025-yuzu-sessions.md))
3. `matcha drift` (mechanical claims built and meets its bar on Anchor, and check comments built, see ADRs [0015](docs/decisions/0015-drift-mechanical-claims.md) and [0016](docs/decisions/0016-drift-check-comments.md)) and `matcha ledger` (built, see [ADR 0017](docs/decisions/0017-ledger.md))
4. `yuzu probe` (built, see [ADR 0018](docs/decisions/0018-yuzu-probe.md))
5. MCP servers and a Claude Code plugin with skills and hooks (built, see [ADR 0019](docs/decisions/0019-mcp-and-plugin.md))
6. `yuzu whoami`, `yuzu wait`, `yuzu sessions`, and `matcha eval`, from friction in real sessions (built, see ADRs [0022](docs/decisions/0022-yuzu-whoami.md) to [0025](docs/decisions/0025-yuzu-sessions.md))
7. `ube`: the change in hand (built, see [its design](docs/ube/design.md) and [ADR 0029](docs/decisions/0029-ube.md))
8. `taro`: the forge and the team (built, see [its design](docs/taro/design.md) and [ADR 0030](docs/decisions/0030-taro.md))

Nothing is announced or published to crates.io until the first two steps prove useful ([ADR 0004](docs/decisions/0004-prove-before-announcing.md)).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). AI agents working in this repo start with [AGENTS.md](AGENTS.md), which also says how tests are split: unit tests drive the code, integration tests pin what users rely on. Labeled answers for matcha's heuristics live in [evals/](evals/README.md).

## License

[Reciprocal Ethical AI License (REAL) 1.0](LICENSE.md). It's source-available, not OSI open source: you can use, change, and share the code, changes stay under the same license, and training a model on it means releasing that model's weights and training pipeline under REAL too. See [ADR 0038](docs/decisions/0038-real-license.md).
