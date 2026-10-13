# Design: mochi-donut

<!-- matcha: links-only -->
<!-- It's a proposal, and its examples quote other repos, so `matcha drift` checks only its links. -->

- **Status:** Accepted for matcha and yuzu, which are built. ube and taro are proposed.
- **Date:** 2026-10-12, updated 2026-10-13
- **Tools:** `matcha` (what the repo knows), `yuzu` (what the user's environment is), and, proposed, `ube` (the change in hand) and `taro` (the forge and the team)

This doc is the design across the tools: why they exist, how they're split, how agents find them, and how they roll out. Each tool's own design, usage, and configuration are in its folder: [matcha/](matcha/README.md), [yuzu/](yuzu/README.md), [ube/](ube/README.md), [taro/](taro/README.md).

## Summary

AI agents do more and more of the work in many repos, one session at a time, with no memory between sessions. They spend much of each session rediscovering things the repo already "knows": how things are usually done here, which files change together, whether the docs are still true, which environment a command will touch, and what they actually checked versus assumed. This doc proposes six capabilities, packaged as two command-line tools, that answer those questions with evidence, for any repo, in a form agents can use directly.

| Command | Tool | Question it answers |
|---|---|---|
| [`census`](matcha/design.md#census-how-is-this-usually-done-here) | `matcha` | How does this repo usually do X? |
| [`recipe`](matcha/design.md#recipe-what-else-changes-when-i-change-this) | `matcha` | When I change this, what else usually changes? |
| [`drift`](matcha/design.md#drift-are-the-docs-still-true) | `matcha` | Are the docs' claims about the code still true? |
| [`ledger`](matcha/design.md#ledger-what-did-i-verify-and-what-did-i-assume) | `matcha` | What did I verify, and what did I assume? |
| [`check`](yuzu/design.md#yuzu-check-what-will-this-command-touch) | `yuzu` | What will this command touch: mine, work, production? |
| [`probe`](yuzu/design.md#yuzu-probe-does-it-work-from-where-the-user-is) | `yuzu` | Does it work from where the user is, not just from my shell? |

Later additions, from friction in real sessions, are under [Added after the design](#added-after-the-design).

Why two tools and not one or six is under [Packaging](#packaging-two-tools-not-one-or-six). How agents learn the tools exist is under [How agents find the tools](#how-agents-find-the-tools).

We'd build `census` and `recipe` first, test them against Anchor's history, and decide on the rest from what we learn.

## Decisions

Recorded as ADRs in [decisions/](decisions/):

- [0001](decisions/0001-own-open-source-repo.md) An open-source monorepo of its own
- [0002](decisions/0002-rust-and-cargo.md) Rust, built with Cargo, not Bazel
- [0003](decisions/0003-names.md) Names from mochi donuts: `matcha` for the repo, `yuzu` for the environment
- [0004](decisions/0004-prove-before-announcing.md) Prove the tools are useful before announcing them
- [0005](decisions/0005-mit-license.md) MIT license

## Background

Every problem below happened in a single working session on 2026-10-12, in Anchor (a private Rust and TypeScript app for a small insurance agency, and the first test bed for these tools). In that session an agent wrote the repo's coding guidelines and set up OpenBao on a home k3s cluster.

- **Guessing the house style.** To write coding guidelines that match the code, the agent ran about a dozen `git grep ... | wc -l` checks. Several overturned its first guess: the code spells it `cancelled` (98 times, `canceled` never); most forms keep a draft in `useState` (19 files) rather than React Hook Form (1 file); IDs are plain `Uuid`, not the newtypes the old conventions doc required.
- **Docs that had drifted.** `engineering/conventions.md` asked for ID newtypes (`ClientId(Uuid)`) that the code never adopted. Nothing noticed. Agents trust docs more than anything else, so a wrong doc is worse than none.
- **Steps that are easy to miss.** `AGENTS.md` keeps a hand-written list of "mechanics that are easy to miss". The history already knows them: every one of the 14 commits on `main` that changed `openapi.rs` also changed `frontend/src/api/openapi.json`.
- **The wrong environment.** The default `kubectl` context was a work cluster, one of about 100 work and production contexts in the same kubeconfig. The agent noticed only because the credentials had expired. One `kubectl apply` could have gone to the wrong place.
- **"It works for me."** The agent's `curl` reached OpenBao three times while the owner's Chrome reported `ERR_ADDRESS_UNREACHABLE`. The causes (a macOS Local Network permission, then a Chrome update that reset it) were invisible from the agent's shell, and each guess cost the owner a round trip.
- **Assumptions written as facts.** While writing the test-data guideline, the agent wrote rules it hadn't checked (FEINs use a `00-` prefix; terms always start on the first of a month) and caught them only by going back to verify.

None of these are specific to Anchor. Any agent in any repo has the same blind spots.

## Goals

- Answer each question with **evidence from the repo or the environment**: counts, examples, commits, and command output. Never opinion.
- **`matcha` works in any git repo** with no setup beyond installing the binary. Repo-specific knowledge is optional configuration, not a requirement. **`yuzu` works in any directory** after a one-time declaration of the user's environments.
- Be **built for agents first**: every command has `--json` output, stable exit codes, and short, plain text output for people.
- Be **fast enough to run on every task**: answers in about a second on a repo the size of Anchor, after a one-time index.
- **Fail safe**: when a tool doesn't know, it says so rather than guessing.

## Non-goals

- Replacing linters, type checkers, or tests. These tools inform; CI enforces.
- Generating code. `recipe` says which files usually change; it doesn't write them.
- A hosted service. Everything runs locally against local data.
- Understanding every language deeply on day one. Start with text and git history; add syntax-aware matching where it pays.

## Users

- **Agents** (Claude Code and others) during a task, through the CLI, through hooks that run the tools automatically, and through an MCP server.
- **The owner**, reading the same output in a terminal or in a PR.

## Design

### Packaging: two tools, not one or six

The six capabilities split by **what they know about**, and that decides the packaging:

| Tool | Knows about | Commands | Scope | Setup |
|---|---|---|---|---|
| **`matcha`** | The repo: its files, history, and docs | `census`, `recipe`, `drift`, `ledger` | Per repo, index under `.matcha/` | None; optional `.matcha.toml` |
| **`yuzu`** | The user's environment: contexts, accounts, hosts, browser | `yuzu` (status), `check`, `probe` | Per user, works in any directory | A one-time declaration of which environments are whose |

Why not one bundle:

- **The guard must be small and trustworthy.** `yuzu check` runs before every shell command, so it has to start in milliseconds and be simple enough to audit. Bundled with an indexer and browser automation, it would be slower and harder to trust.
- **Different scope.** `matcha` answers questions about one repo. "Is this command about to hit production?" matters just as much outside any repo (a folder of homelab manifests often isn't a git repo at all).
- **Different setup and lifecycle.** `matcha` works with no setup in any repo. `yuzu` is useless until the user declares their environments, and that declaration belongs to the user, not to a repo.

Why not six separate tools:

- `census`, `recipe`, `drift`, and `ledger` all read the same index of files and history. Separate binaries would mean four indexes, four installs, and four sets of conventions for an agent to learn.
- `probe` belongs with `yuzu` because both are about the user's machine and network, not the repo, and both share the same environment facts (DNS, proxies, VPNs, which browser).

Each tool is one Rust binary in the `mochi-donut` workspace ([ADR 0002](decisions/0002-rust-and-cargo.md)). They share conventions (flags, `--json` shapes, exit codes) but no code they don't need.

### Interfaces: a CLI first, with hooks and MCP on top

| Interface | What it's for | Which commands |
|---|---|---|
| **CLI** (the core) | Every agent with a shell can use it, and so can people, CI, and git hooks. Easy to test. | All |
| **Hooks** | Checks that must run whether or not the agent remembers | `yuzu check` before every shell command; `recipe --check` and `ledger check --diff` before a commit; `drift` in CI |
| **MCP server** (thin layer over the CLI) | Typed inputs and outputs, and a place in the agent's tool list every session | `census`, `recipe`, `ledger` especially; `ledger` is something the agent writes to as it works |

Not a daemon, and not an IDE plugin. A background process to keep `matcha`'s index warm could come later if speed needs it; nothing in this design depends on one.

### The tools in detail

Each tool's commands, with their problems, designs, bars, and status, are in its own design doc:

- [matcha/design.md](matcha/design.md): the index, `census`, `recipe`, `drift`, `ledger`, and `eval`
- [yuzu/design.md](yuzu/design.md): `check`, `probe`, `whoami`, `wait`, and `sessions`
- [ube/design.md](ube/design.md) (proposed): `plan`, `run`, `done`, and `mutate`
- [taro/design.md](taro/design.md) (proposed): `ready`, `reviews`, `owners`, `flaky`, and `cleanup`

### Two more tools: ube and taro (proposed)

Building and merging mochi-donut with these tools showed two kinds of knowledge neither has. They follow the same split by what a tool knows, and each has a different trust boundary, which is why they're separate:

| Tool | Knows about | Why not in matcha or yuzu |
|---|---|---|
| **`ube`** | The change in hand: which checks apply, whether they ran on this exact tree, what failed | It runs the repo's own code (tests, builds). matcha only reads, and must stay safe to call anywhere. |
| **`taro`** | The forge and the team: PR readiness, reviews, owners, flaky checks, merged branches | Everything it does is a network call to a forge. yuzu's check runs before every command and must stay local. |

Both are proposals with measurable bars, per [ADR 0004](decisions/0004-prove-before-announcing.md); see [ADR 0028](decisions/0028-propose-ube-and-taro.md).

## How agents find the tools

An agent reliably uses a tool it has only been told about some of the time. So discovery is layered from strongest to weakest, and **anything safety depends on sits in the first layer.**

1. **Hooks: the agent doesn't need to know.** `yuzu check` and the pre-commit checks run on their own. The agent learns about them the first time one stops it, from the message it gets back.
2. **Output that teaches.** Every message says what to do next: a blocked command names the safe alternative ("Re-run with `--context k3s-pis`"), and a `recipe` warning names the fix (`make openapi`).
3. **MCP tool descriptions.** With the MCP server installed, the tools are in the agent's tool list every session. Descriptions are written as triggers, not features: "Use before copying a pattern or writing a guideline."
4. **A short trigger table in the repo's agent instructions** (`AGENTS.md`, `CLAUDE.md`), written as "when X, run Y", not as a manual:
   ```
   Before copying a pattern or writing a guideline   matcha census
   Before a change you haven't made here before      matcha recipe --like "<what you're doing>"
   Before committing                                 (runs automatically)
   ```
5. **A Claude Code skill** with fuller instructions, loaded only when its description matches the task, so it costs no context the rest of the time.
6. **Self-description.** `matcha agent-guide` and `yuzu agent-guide` print a compact guide written for agents, so any agent can learn either tool with one command.

**The principle: agents may use a tool for convenience, but safety never depends on them remembering to.** `census` and `recipe --like` make an agent better when it uses them. `yuzu check` and the pre-commit checks have to work when it doesn't.

**Status (2026-10-12): built.** `matcha mcp` and `yuzu mcp` serve each tool's commands over stdio, as a thin layer that reruns the CLI with `--json`; their descriptions are triggers. This repo is a Claude Code plugin marketplace: `mochi-donut` carries the MCP servers and a skill per tool, and `mochi-donut-hooks` carries yuzu's check and matcha's recorder, as a separate install because it changes how commands are approved. The README has the trigger table. See [ADR 0019](decisions/0019-mcp-and-plugin.md).

## Added after the design

Using the tools in real sessions showed four gaps the design didn't plan for. Each is built and has its own ADR.

| Command | Question it answers | Why |
|---|---|---|
| [`yuzu whoami`](yuzu/design.md#yuzu-whoami-which-account-will-this-act-as) | Which account will this act as? | A stale `GH_TOKEN` silently overrode a working `gh` login and blocked a PR twice ([ADR 0022](decisions/0022-yuzu-whoami.md)) |
| [`yuzu wait`](yuzu/design.md#yuzu-wait-is-it-done-yet) | Is it done yet? | Waiting for CI took four tries with `gh`'s watchers and hand-written loops ([ADR 0023](decisions/0023-yuzu-wait.md)) |
| [`matcha eval`](matcha/design.md#eval-are-the-answers-still-right) | Are the answers still right? | Heuristics were tuned by judging findings by hand; now the judgments are labels and the result is a number ([ADR 0024](decisions/0024-matcha-eval.md)) |
| [`yuzu sessions`](yuzu/design.md#yuzu-sessions-how-do-agents-use-these-tools) | How do agents use these tools? | ADR 0004's trial needs unprompted-use counts and every hook ask or deny to review ([ADR 0025](decisions/0025-yuzu-sessions.md)) |

## Rollout

| Phase | Build | Prove it on |
|---|---|---|
| 1 | `matcha`: the index, `census`, `recipe` | Anchor: the guideline claims (census) and a replay of the last 20 feature PRs (recipe) |
| 2 | `yuzu` status and `check`, with its hook | The homelab and the work kubeconfig on this Mac |
| 3 | `matcha drift` (mechanical claims, then check comments), `matcha ledger` | Anchor's docs, including the pre-guidelines version |
| 4 | `yuzu probe` | Today's OpenBao session, replayed |
| 5 | MCP server, skill, `agent-guide`; try both tools on a second repo | A repo with a different language and history |
| 6 | `yuzu whoami`, `wait`, `sessions`; `matcha eval` (built) | The friction that prompted each, replayed; `evals/` for matcha's heuristics |
| 7 | `ube` (proposed) | Anchor, this repo, and one other: the last 20 PRs' CI failures |
| 8 | `taro` (proposed) | Two team repos' last 30 merged PRs |

After each phase, decide from the results whether to continue, change course, or stop. Each decision gets an ADR.

## Risks

- **False confidence.** A tool that's wrong but sounds sure is worse than no tool. Mitigation: always show the evidence (counts, commits, matches) and the sample size, and stay quiet below a confidence threshold.
- **Stale index.** Mitigation: incremental updates on every run; report the index's age in `--json` output.
- **Noisy checks get ignored.** Mitigation: `recipe --check` and `drift` start report-only, and become blocking only after their false-alarm rates are measured.
- **`yuzu check` gets bypassed.** It can only check commands it sees. Mitigation: run it as a hook, not a habit; treat unknown targets as risky.
- **Privacy in `yuzu probe`.** Mitigation: headless, a temporary profile copy, no cookies or passwords, nothing leaves the machine.
- **Maintenance cost.** Six capabilities grew to fifteen commands, with ten more proposed. Mitigation: binaries split by what they know, one index for the repo commands; build in phases, measure use with `yuzu sessions`, and stop when the value runs out.
- **Agents don't use the optional commands.** Mitigation: the layered discovery above, and measuring use (see `ledger`'s "done when"). If agents still don't reach for `census` or `recipe --like`, trigger them from hooks too.

## Alternatives considered

- **Existing tools, combined:** `git grep` and ripgrep (counts, but no notion of alternatives or examples), ast-grep and Semgrep (structural rules you must write first; no history), code-ownership and co-change analyzers such as `git-of-theseus` and CodeScene (history analysis for people, not checks for agents), link checkers such as lychee (one kind of drift only), `kube-ps1` and `kubectx` (show the context; don't classify or block it). Each covers a sliver, none produce evidence in a form agents can act on, and none record what an agent verified.
- **Hand-written checklists and conventions docs** (what Anchor has today): useful but they drift, as `conventions.md` showed. The tools are meant to keep these honest, not replace them.
- **One binary for everything:** simpler to install, but it puts an indexer and browser automation in the path of a check that runs before every command. See [Packaging](#packaging-two-tools-not-one-or-six).
- **Six separate tools:** each is simpler alone, but four of them would duplicate the same index, and agents would have six tools to discover instead of two.
- **MCP only, no CLI:** good discovery, but no use from git hooks, CI, or a person's terminal, and harder to test.
- **Repo-specific scripts** (`cargo xtask` commands for Anchor): quicker to write for one repo, but every repo has these problems. A general tool with optional repo config serves Anchor and everything else.

## Open questions

1. ~~**`yuzu probe` scope.** Chrome only at first, or Chrome and Safari from the start?~~ Chromium-based browsers only; Safari has no headless mode ([ADR 0018](decisions/0018-yuzu-probe.md)).
2. **Sharing the environment declarations.** Should yuzu's file be per user only, or can a repo ship its own declarations (for example, "this repo deploys to the production host")?
3. Each tool's own open questions are at the end of its design doc.
