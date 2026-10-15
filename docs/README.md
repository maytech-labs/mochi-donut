# mochi-donut docs

| Where | What |
|---|---|
| [design.md](design.md) | The design across the tools: why they exist, how they're split, how agents find them, rollout, and risks |
| [matcha/](matcha/README.md) | What the repo knows: `census`, `recipe`, `drift`, `ledger`, `eval` |
| [yuzu/](yuzu/README.md) | What the environment is: `status`, `check`, `probe`, `whoami`, `wait`, `sessions` |
| [ube/](ube/README.md) | The change in hand: `plan`, `run`, `done`, `mutate` |
| [taro/](taro/README.md) | The forge and the team: `ready`, `reviews`, `owners`, `flaky`, `cleanup` |
| [decisions/](decisions/) | Every decision, as an ADR, never rewritten. Copy [0000-template.md](decisions/0000-template.md) to add one. |

Each tool's folder has the same pages, so they're easy to find:

| Page | Holds |
|---|---|
| `README.md` | What the tool is, its commands at a glance, and links |
| `design.md` | The problem each command solves, how it works, its "done when" bar, its status, and open questions |
| `usage.md` | Every command and flag, with examples and exit codes |
| `config.md` | Configuration and the files the tool writes, where there are any |
| `agents.md` | How agents reach it: hooks, MCP tools, the Claude Code skill, and trigger lines |

taro has no `config.md`: it has nothing to configure.

## Decisions

- [0001 An open-source monorepo of its own](decisions/0001-own-open-source-repo.md)
- [0002 Rust, built with Cargo, not Bazel](decisions/0002-rust-and-cargo.md)
- [0003 Names from mochi donuts: matcha for the repo, yuzu for the environment](decisions/0003-names.md)
- [0004 Prove the tools are useful before announcing them](decisions/0004-prove-before-announcing.md)
- [0005 MIT license](decisions/0005-mit-license.md)
- [0006 Terse conventional commits, no tool attribution](decisions/0006-commit-messages.md)
- [0007 Phase 1 index: ask git, compute in memory](decisions/0007-phase-1-index.md)
- [0008 How recipe decides a file is missing, and how we measure it](decisions/0008-recipe-scoring.md)
- [0009 A release profile tuned for small and fast](decisions/0009-release-profile.md)
- [0010 `make install` into /usr/local/bin](decisions/0010-make-install.md)
- [0011 This repo is its own Homebrew tap](decisions/0011-homebrew-tap.md)
- [0012 recipe needs six earlier changes; commit weighting didn't help](decisions/0012-recipe-support-six.md) (amended)
- [0013 How yuzu check finds targets and decides](decisions/0013-yuzu-check.md) (amended)
- [0014 yuzu runs as a Claude Code hook that's silent when it allows](decisions/0014-claude-code-hook.md) (amended)
- [0015 drift checks mechanical claims first, and stays quiet when unsure](decisions/0015-drift-mechanical-claims.md) (amended)
- [0016 drift check comments: `no-match` and `match`, and suggestions with evidence](decisions/0016-drift-check-comments.md)
- [0017 ledger: local per-branch files, evidence required, matched by shared words](decisions/0017-ledger.md) (amended)
- [0018 yuzu probe: curl and headless Chrome with an empty profile, and a few exact rules](decisions/0018-yuzu-probe.md) (amended)
- [0019 MCP as `<tool> mcp` over the CLI, and a two-plugin marketplace in this repo](decisions/0019-mcp-and-plugin.md)
- [0020 yuzu check covers infrastructure-as-code tools and cloud CLIs](decisions/0020-yuzu-infra-and-cloud-clis.md)
- [0021 Changes from trying the tools on real questions](decisions/0021-usability-from-trying-the-tools.md)
- [0022 yuzu whoami: which account each tool acts as, offline unless asked](decisions/0022-yuzu-whoami.md)
- [0023 yuzu wait: one command for waiting on CI, ports, URLs, files, and commands](decisions/0023-yuzu-wait.md)
- [0024 matcha eval: labeled answers in spec files, for any command that prints JSON](decisions/0024-matcha-eval.md)
- [0025 yuzu sessions: tool use from agent transcripts, and a log of the hook's asks and denies](decisions/0025-yuzu-sessions.md)
- [0026 Two kinds of test: unit tests drive the code, integration tests pin the contract](decisions/0026-two-kinds-of-test.md)
- [0027 A docs folder per tool, with the same pages in each](decisions/0027-docs-per-tool.md)
- [0028 Propose ube (the change in hand) and taro (the forge and the team)](decisions/0028-propose-ube-and-taro.md)
- [0029 ube: checks by package from what the repo declares, results by tree, line-based mutations](decisions/0029-ube.md)
- [0030 taro: gh for the forge, conventions from merged PRs, cleanup by proof](decisions/0030-taro.md)
- [0031 ube measures a branch from where it started, and plans the Rust lints the repo runs](decisions/0031-ube-fixes.md)
- [0032 taro takes required checks from isRequired, and blocks when GitHub does](decisions/0032-taro-fixes.md)
- [0033 ube reads CI by job and directory, measured by replaying merged changes](decisions/0033-ube-ci-steps-and-replay.md)
- [0035 Merge pull requests when CI passes](decisions/0035-merge-on-green.md)
- [0036 yuzu asks before anything changes its own config](decisions/0036-yuzu-guards-its-config.md)
- [0037 `yuzu env add` declares a target, and asks name it](decisions/0037-yuzu-env-add.md)
