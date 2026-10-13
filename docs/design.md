# Design: mochi-donut

<!-- matcha: links-only -->
<!-- It's a proposal, and its examples quote other repos, so `matcha drift` checks only its links. -->

- **Status:** Accepted. `census`, `recipe`, `drift`, `yuzu` status, and `yuzu check` are built; each command's section says how far.
- **Date:** 2026-10-12
- **Tools:** `matcha` (what the repo knows) and `yuzu` (what the user's environment is)

## Summary

AI agents do more and more of the work in many repos, one session at a time, with no memory between sessions. They spend much of each session rediscovering things the repo already "knows": how things are usually done here, which files change together, whether the docs are still true, which environment a command will touch, and what they actually checked versus assumed. This doc proposes six capabilities, packaged as two command-line tools, that answer those questions with evidence, for any repo, in a form agents can use directly.

| Command | Tool | Question it answers |
|---|---|---|
| [`census`](#census-how-is-this-usually-done-here) | `matcha` | How does this repo usually do X? |
| [`recipe`](#recipe-what-else-changes-when-i-change-this) | `matcha` | When I change this, what else usually changes? |
| [`drift`](#drift-are-the-docs-still-true) | `matcha` | Are the docs' claims about the code still true? |
| [`ledger`](#ledger-what-did-i-verify-and-what-did-i-assume) | `matcha` | What did I verify, and what did I assume? |
| [`check`](#yuzu-check-what-will-this-command-touch) | `yuzu` | What will this command touch: mine, work, production? |
| [`probe`](#yuzu-probe-does-it-work-from-where-the-user-is) | `yuzu` | Does it work from where the user is, not just from my shell? |

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

### `matcha`: what the repo knows

#### The index

`matcha`'s four commands sit on one local index. Phase 1 builds it in memory from `git` on each run ([ADR 0007](decisions/0007-phase-1-index.md)); the persistent form described here, under `.matcha/` (ignored by git) or a user cache directory, comes when repos need it:

- **Files:** paths, languages, sizes, generated or vendored markers.
- **History:** commits with their changed files, messages, and dates, from `git log --name-status`. Squash-merged PRs count as one change.
- **Text search:** backed by ripgrep's engine for speed.
- **Syntax-aware search:** tree-sitter grammars (through ast-grep's matcher), so a query can mean "functions that call `require_write`" rather than "lines containing `require_write`".

The index updates incrementally from git: new commits and changed files only.

Subcommands share flags: `--json`, `--repo <path>`, `--since <date>`, `--limit <n>`.

Optional repo configuration lives in `.matcha.toml`: named patterns and protected paths. Every command works without it.

#### `census`: how is this usually done here?

**Problem.** Agents copy whatever code they happen to read first, so the minority pattern spreads, and guidelines get written from guesses.

**What it does.** Counts the ways the repo does something and shows the best example of each.

```
$ matcha census "form state" --in frontend/src/features
  draft in useState + <thing>FormModel.ts   19 files   features/holders/HolderForm.tsx
  react-hook-form + zod                      1 file    features/clients/ClientForm.tsx

$ matcha census --spelling cancel
  cancelled   98   (Rust 66, TypeScript 20, SQL 6, docs 6)
  canceled     0
```

**How it works.**
- A query is either a **named pattern** (from `.matcha.toml`, or a built-in set for common questions such as form state, error handling, and test layout) or a set of **alternatives** given on the command line (`--alt 'useForm(' --alt 'useState<.*Draft>'`).
- It counts matches per alternative by file and by area, and ranks examples by how typical they are: recent, short, and close to the asker's working directory.
- `--spelling <stem>` finds the spelling variants of a word across identifiers, strings, and docs.
- `--trend` shows whether each alternative is growing or shrinking over time, so "the old way" and "the new way" are clear.

**Output.** Each alternative with its count, its share, and up to three example locations. With `--json`, the same as data.

**Done when.** On Anchor, every claim in `docs/guidelines/` that describes a pattern can be confirmed or refuted by one `census` command.

**Status (2026-10-12): built.** `--alt` and `--spelling` work. On Anchor it confirmed the forms guideline (`\b\w+Draft\b` in 17 files, 94%, against `useForm` in 1) and the spelling rule (`cancelled` 105 times, `canceled` never) in under a second. Named patterns and `--trend` aren't built yet.

#### `recipe`: what else changes when I change this?

**Problem.** Many changes need companion edits that are easy to forget (regenerated files, registrations, tests). Repos keep hand-written checklists for them, and the checklists go stale.

**What it does.** Learns from history which files change together, and uses that two ways. (The first example's counts are illustrative; the second uses real numbers from Anchor's history.)

```
$ matcha recipe --like "add an endpoint"
  Based on 9 similar changes:
    backend/crates/anchor-api/src/routes/<resource>.rs     9/9
    backend/crates/anchor-api/src/openapi.rs               9/9
    frontend/src/api/openapi.json, schema.d.ts             9/9
    backend/tests/contract/<resource>.rs                   8/9
    frontend/src/api/queries/<resource>.ts                 7/9

$ matcha recipe --check            # against the working tree
  openapi.rs changed but frontend/src/api/openapi.json didn't.
    14 of 14 earlier changes to openapi.rs changed both.
    Usual fix: make openapi
```

**How it works.**
- **Co-change statistics:** for each pair of files (and each pair of path patterns, such as `routes/*.rs` and `tests/contract/*.rs`), how often they change in the same commit, as support and confidence. Patterns come from grouping paths that share structure.
- **Similar changes:** `--like` finds past commits by their message and their changed files, then reports what those commits had in common. With no message, it uses the current diff to find similar past changes.
- **Fix hints:** when a missing companion file is generated, `recipe` looks for the command that regenerates it: in the `Makefile` and `package.json` scripts, and in commit messages and docs that name both.
- `--check` runs before a commit and only reports rules with high confidence and enough history (defaults: 90% of at least 6 changes), so it stays quiet unless it's sure.

**Done when.** Replaying Anchor's last 20 feature PRs with the final commit's companion files removed, `recipe --check` names the missing files with at least 80% recall and almost no false alarms (precision of at least 90%).

**Status (2026-10-12): built, below the bar.** `--check`, `--commit`, `--like`, and `--replay` work. Measured with `--replay` at the defaults: 100% precision on Ghostty, 97% on vim, 85% on Anchor, and 78% on MoltenVK, against the 90% bar. Precision depends on how a repo commits more than on the scoring, so the bar is per repo. The recall bar turned out to need labeled companion files to measure fairly. Scoring and measurement are in [ADR 0008](decisions/0008-recipe-scoring.md); the defaults and what didn't help are in [ADR 0012](decisions/0012-recipe-support-six.md). Path patterns in `--check` aren't built yet.

#### `drift`: are the docs still true?

**Problem.** Docs make claims about code, and nothing checks them. When code changes, the doc keeps saying the old thing, and the next agent follows it.

**What it does.** Finds claims in the docs that can be checked against the code, checks them, and reports the ones that are no longer true.

Run against Anchor's docs as they were before the guidelines PR:

```
$ matcha drift
  docs/engineering/conventions.md:13
    "Use newtypes for IDs (`ClientId(Uuid)`)"
    FAILS: no type `ClientId` exists; IDs are `Uuid` in every crate
  docs/architecture/backend.md
    "`anchor-api::files::FileStore`"  OK
  (counts of checked and failing claims, then the details)
```

**How it works.**
- **Mechanical claims** come from the text with no help: code spans that name files, paths, functions, types, commands, and `make` targets must exist; links and anchors must resolve; numbers stated beside a path ("13 ADRs") must match.
- **Rule claims** ("never X outside Y", "every Z has W") are marked in the doc with a small comment that holds the check, so the check lives next to the sentence it proves:
  ```markdown
  <!-- matcha: no-match 'Utc::now\(|Local::now\(' except 'clock.rs' -->
  Never call `Utc::now()` outside a `Clock` implementation.
  ```
- **Suggesting checks:** `matcha drift --suggest` proposes check comments for sentences that look like rules, for a person or agent to review and accept. It never adds them silently.
- Runs in CI as a report first, and as a failing check once the false-alarm rate is low.

**Done when.** Run on Anchor's docs as they were before the guidelines PR, it flags the ID-newtype claim in `engineering/conventions.md`.

**Status (2026-10-12): mechanical claims built, and the bar is met.** Links, anchors, and code spans naming paths, identifiers, `make` targets, and `npm run` scripts are checked. On Anchor before the guidelines PR it flags `conventions.md:13`, `ClientId(Uuid)`, plus 10 other failures, 9 of them real drift. On Ghostty and this repo it finds nothing; on IntelliJ and Ghidra about 31 of 50 and 7 of 19 are real, and the rest are symbols naming code outside the repo. What counts as a claim, the `skip` and `links-only` comments, and the measurements are in [ADR 0015](decisions/0015-drift-mechanical-claims.md). Check comments (`no-match` and `match`) and `--suggest` are built too; on Anchor the design's clock rule catches a contract test calling `Utc::now()` directly. Counts beside a path aren't built, because what they count is a guess. See [ADR 0016](decisions/0016-drift-check-comments.md).

#### `ledger`: what did I verify, and what did I assume?

**Problem.** Agents mix checked facts with guesses, then write both into docs and PRs with the same confidence. The next session can't tell which is which.

**What it does.** Keeps a per-task record of claims, each marked verified (with the evidence) or assumed, and flags assumed claims before they reach a commit.

```
$ matcha ledger add verified "Traefik listens on all three nodes" --evidence "kubectl get svc -A: 10.0.0.127,10.0.0.24,10.0.0.8"
$ matcha ledger add assumed "FEINs in tests use a 00- prefix"
$ matcha ledger check --diff
  docs/guidelines/testing/test-data.md:11 states an assumed claim:
    "FEINs use the 00- prefix"   (assumed, no evidence)
```

**How it works.**
- Entries live in `.matcha/ledger/<branch>.jsonl`: the claim, its status, the evidence (a command and an excerpt of its output, or a file and line), and a time.
- Agents add entries through the CLI or an MCP tool. A hook can record commands and outputs automatically, so evidence is attached without extra work.
- `check --diff` matches assumed claims against added lines in the diff (by text similarity) and lists the ones that appear in docs or comments.
- Distinct from ADRs: **decisions go in ADRs, facts go in the ledger.** A ledger entry that turns out to be a decision is promoted to an ADR.
- When a branch merges, its ledger can be summarized into the PR description ("Verified: ..., Assumed: ...") and archived.

**Done when.** Agents use it without being reminded in at least most sessions, and assumed claims stop appearing unmarked in merged docs.

### `yuzu`: what the user's environment is

#### `yuzu check`: what will this command touch?

**Problem.** Agents run commands against whatever the shell is pointed at. The current Kubernetes context, cloud profile, SSH host, or git remote might be a production system the user never meant to involve. Existing tools show the current context; none of them know which contexts are whose, or stop an agent before it acts.

**What it does.** Classifies every target a command would reach, using a declaration the user writes once, and gates risky ones.

```
$ yuzu
  kube context   work-nonprod-us-east-1   WORK (nonprod)
  aws profile    default                      unset
  git remote     upstream → github.com/trammel-may/anchor   MINE
  
$ yuzu check -- kubectl apply -f openbao.yaml
  BLOCKED: kubectl would use context work-nonprod-us-east-1 (WORK).
  Your declared homelab context is k3s-pis. Re-run with --context k3s-pis, or allow it explicitly.
```

**How it works.**
- A user-level file (`~/.config/yuzu/environments.toml`) declares environments with patterns and a class: `mine`, `work`, or `production`, plus whether agents may write to it.
  ```toml
  [[env]]
  name = "homelab"
  class = "mine"
  kube_contexts = ["k3s-pis"]
  ssh_hosts = ["pi-1", "pi-2", "pi-3"]

  [[env]]
  name = "work"
  class = "production"
  kube_contexts = ["*-prod-*"]
  agents = "read-only"
  ```
- `yuzu check -- <command>` resolves what the command would act on (the effective kube context and namespace, `--context` flags, `KUBECONFIG`, AWS and GCP profiles, `ssh` hosts, `git push` remotes, `docker` contexts) and compares it with the declarations.
- **The intended use is as a pre-command hook** (for example, a Claude Code `PreToolUse` hook on Bash), so the check runs whether or not the agent remembers it. Read-only commands on work environments pass; writes to anything not declared `mine` need explicit approval.
- Unknown targets count as risky.

**Done when.** With the declarations above, an agent session that tries a write against any context other than the homelab is stopped before the command runs, and reads still work.

**Status (2026-10-12): built, on trial.** `yuzu`, `yuzu check`, and `yuzu hook claude-code` work for `kubectl`, `helm`, `ssh`, `scp`, `rsync`, `git push`, and `aws`. Writes outside `mine` ask the user rather than being denied. Replaying 2,118 past Bash commands from this Mac stopped one, correctly, and blocked nothing it shouldn't have. The two-week hook trial from [ADR 0004](decisions/0004-prove-before-announcing.md) starts when the hook is installed. Details in [ADR 0013](decisions/0013-yuzu-check.md) and [ADR 0014](decisions/0014-claude-code-hook.md).

#### `yuzu probe`: does it work from where the user is?

**Problem.** An agent verifies from its own shell, but the user hits the service from a browser with its own DNS, permissions, proxy, VPN, certificate store, and cache. "Works for me" and "can't be reached" can both be true, and the agent can't see the difference.

**What it does.** Runs a check the way the user's browser would, and reports the exact failure.

```
$ yuzu probe https://openbao.local/ui/
  terminal (curl)           200   10.0.0.8   cert trusted
  Chrome (user profile)     FAIL  ERR_ADDRESS_UNREACHABLE
  likely cause: macOS Local Network permission for Google Chrome
    (Chrome updated to 155.0.8059.40 at 14:58; permission often resets on update)
```

**How it works.**
- Drives the user's installed browser in headless mode through the Chrome DevTools Protocol (and WebKit for Safari where possible), with a temporary copy of the profile's relevant settings rather than the profile itself.
- Captures the network error code, the DNS answers (IPv4 and IPv6), the certificate chain and its trust result, and the proxy decision.
- A small set of **diagnosis rules** maps symptoms to likely causes: "terminal succeeds, browser `ERR_ADDRESS_UNREACHABLE` on a private address" points at Local Network permission; "IPv6 refused, IPv4 OK" points at the name's AAAA records; a recent browser update is reported when present.
- Never reads cookies, history, or saved passwords, and never sends anything off the machine.

**Done when.** Replaying today's OpenBao session, `yuzu probe` names the Local Network permission on the first failure, without a round trip to the user.

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

## Rollout

| Phase | Build | Prove it on |
|---|---|---|
| 1 | `matcha`: the index, `census`, `recipe` | Anchor: the guideline claims (census) and a replay of the last 20 feature PRs (recipe) |
| 2 | `yuzu` status and `check`, with its hook | The homelab and the work kubeconfig on this Mac |
| 3 | `matcha drift` (mechanical claims, then check comments), `matcha ledger` | Anchor's docs, including the pre-guidelines version |
| 4 | `yuzu probe` | Today's OpenBao session, replayed |
| 5 | MCP server, skill, `agent-guide`; try both tools on a second repo | A repo with a different language and history |

After each phase, decide from the results whether to continue, change course, or stop. Each decision gets an ADR.

## Risks

- **False confidence.** A tool that's wrong but sounds sure is worse than no tool. Mitigation: always show the evidence (counts, commits, matches) and the sample size, and stay quiet below a confidence threshold.
- **Stale index.** Mitigation: incremental updates on every run; report the index's age in `--json` output.
- **Noisy checks get ignored.** Mitigation: `recipe --check` and `drift` start report-only, and become blocking only after their false-alarm rates are measured.
- **`yuzu check` gets bypassed.** It can only check commands it sees. Mitigation: run it as a hook, not a habit; treat unknown targets as risky.
- **Privacy in `yuzu probe`.** Mitigation: headless, a temporary profile copy, no cookies or passwords, nothing leaves the machine.
- **Maintenance cost.** Six capabilities are a lot. Mitigation: two binaries split by what they know, one index for the repo commands; build in phases and stop when the value runs out.
- **Agents don't use the optional commands.** Mitigation: the layered discovery above, and measuring use (see `ledger`'s "done when"). If agents still don't reach for `census` or `recipe --like`, trigger them from hooks too.

## Alternatives considered

- **Existing tools, combined:** `git grep` and ripgrep (counts, but no notion of alternatives or examples), ast-grep and Semgrep (structural rules you must write first; no history), code-ownership and co-change analyzers such as `git-of-theseus` and CodeScene (history analysis for people, not checks for agents), link checkers such as lychee (one kind of drift only), `kube-ps1` and `kubectx` (show the context; don't classify or block it). Each covers a sliver, none produce evidence in a form agents can act on, and none record what an agent verified.
- **Hand-written checklists and conventions docs** (what Anchor has today): useful but they drift, as `conventions.md` showed. The tools are meant to keep these honest, not replace them.
- **One binary for everything:** simpler to install, but it puts an indexer and browser automation in the path of a check that runs before every command. See [Packaging](#packaging-two-tools-not-one-or-six).
- **Six separate tools:** each is simpler alone, but four of them would duplicate the same index, and agents would have six tools to discover instead of two.
- **MCP only, no CLI:** good discovery, but no use from git hooks, CI, or a person's terminal, and harder to test.
- **Repo-specific scripts** (`cargo xtask` commands for Anchor): quicker to write for one repo, but every repo has these problems. A general tool with optional repo config serves Anchor and everything else.

## Open questions

1. **`yuzu probe` scope.** Chrome only at first, or Chrome and Safari from the start?
2. **Sharing the environment declarations.** Should yuzu's file be per user only, or can a repo ship its own declarations (for example, "this repo deploys to the production host")?
