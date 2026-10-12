# mochi-donut

Tools that help AI coding agents work in a repo the way a careful teammate would: with evidence about how things are done here, and without touching the wrong environment.

> **Status: pre-release.** The commands exist and parse, but none do their real work yet. See the [design](docs/design.md) and the [roadmap](#roadmap).

Agents start every session with no memory. They guess the house style, forget the companion edits a change needs, trust docs that have drifted, and run commands against whatever cluster or account the shell happens to point at. mochi-donut answers those questions from the repo's own history and from a declaration of your environments, in a form agents can act on.

Like a pon de ring, it's a ring of separate pieces. Each tool is named after a mochi donut flavor.

| Tool | Knows about | Commands |
|---|---|---|
| **matcha** | The repo: its files, history, and docs | `census` (how is this usually done here?), `recipe` (what else changes with this?), `drift` (are the docs still true?), `ledger` (what did I verify, and what did I assume?) |
| **yuzu** | Your environment: clusters, accounts, hosts, browser | `status` (where am I?), `check` (what will this command touch, and is it allowed?), `probe` (does it work from where the user is?) |

Every command has `--json` output and stable exit codes, and each tool prints a guide written for agents:

```sh
matcha agent-guide
yuzu agent-guide
```

## Install

From source, until the first release:

```sh
cargo install --git https://github.com/trammel-may/mochi-donut mochi-matcha mochi-yuzu
```

The packages are named `mochi-matcha` and `mochi-yuzu`; the commands they install are `matcha` and `yuzu`.

Note: Homebrew has an unrelated `matcha` (an RSS digest tool). If you have both, whichever comes first on your `PATH` wins.

## Roadmap

1. `matcha census` and `matcha recipe`, proven on real repos
2. `yuzu status` and `yuzu check`, run as a pre-command hook
3. `matcha drift` and `matcha ledger`
4. `yuzu probe`
5. An MCP server and agent skills

Nothing is announced or published to crates.io until the first two steps prove useful ([ADR 0004](docs/decisions/0004-prove-before-announcing.md)).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). AI agents working in this repo start with [AGENTS.md](AGENTS.md).

## License

[MIT](LICENSE)
