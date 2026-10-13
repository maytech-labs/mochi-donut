# Changelog

All notable changes are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## Unreleased

### Added

- Cargo workspace with `matcha` and `yuzu`. Every planned command parses and reports that it isn't built yet (exit code 3, or a JSON body with `--json`).
- `matcha agent-guide` and `yuzu agent-guide`.
- `matcha census`: compare alternatives (`--alt`) or spellings (`--spelling`) across tracked files, by language, with examples.
- `matcha recipe`: `--check` the working tree or a past `--commit` for missing companion files, with the usual fix; `--like` for what similar changes touched; `--replay` to measure its own precision on the repo's history.
- `matcha drift`: check that the docs' links, anchors, and code spans naming paths, identifiers, `make` targets, and `npm run` scripts still match the repo. `<!-- matcha: skip -->` and `<!-- matcha: links-only -->` mark what isn't a claim.
- `matcha drift` check comments: `<!-- matcha: no-match '<regex>' except '<path>' -->` beside a sentence proves it on every run. `--suggest` proposes them, each with whether the code follows it today.
- `matcha ledger`: record claims as verified (with evidence) or assumed, per branch, in a local `.matcha/` that ignores itself; `check --diff` flags assumed claims written into docs and comments; `summary` prints them for a PR.
- `matcha hook claude-code`: a `PostToolUse` hook that records Bash commands and their output, for `ledger add verified --cmd`.
- `yuzu` (status): the current kube context, aws profile, and git remotes, and whose they are.
- `yuzu check`: what a command would touch through `kubectl`, `helm`, `ssh`, `scp`, `rsync`, `git push`, and `aws`, and whether it needs the user's approval.
- `yuzu probe <url>`: load a URL from the terminal (curl) and from the user's browser (headless Chrome, Edge, Brave, or Chromium, with an empty temporary profile), and name the likely cause when they differ, such as macOS's Local Network permission.
- `yuzu hook claude-code`: `check` as a Claude Code `PreToolUse` hook.
- `matcha mcp` and `yuzu mcp`: each tool's commands as MCP tools over stdio, with descriptions written as triggers.
- A Claude Code plugin marketplace in this repo: `mochi-donut` (MCP servers and skills) and `mochi-donut-hooks` (yuzu's check and matcha's recorder).
- Design doc and ADRs 0001–0028.
- `make install` (default `/usr/local/bin`) and a Homebrew formula (`brew install --HEAD mochi-donut` from this repo's tap).
- `yuzu whoami`: which account gh, git, ssh, and AWS act as here, and what overrides it; `--verify` asks each service.
- `yuzu wait`: wait for CI (GitHub), a port, a URL, a file, or a command, with a timeout and a clear exit code.
- `matcha eval`: score any command's JSON answers against labeled ones in a spec, pinned to a commit; `evals/` holds this repo's cases.
- `yuzu sessions`: how agents used given tools, from Claude Code transcripts, including unprompted use; the hook now logs its asks and denies for review.

### Changed

- Docs are split per tool: `docs/<tool>/` holds each tool's design, usage, configuration, and agent pages; `docs/design.md` keeps the cross-tool design. ube and taro are proposed there.
- `yuzu check` matches `infra_dirs` like `~/infra/*` when the home directory's path involves a symlink (`/var` and `/private/var` on macOS); before, it asked.
- Testing guidelines in AGENTS.md: unit tests drive the code, integration tests pin the contract and change only with it.
- `census` leaves out lockfiles and minified bundles, says how many matching files are tests (`--no-tests` drops them), and flags a pattern narrower than its word.
- `recipe --like` keeps files most similar commits changed, matches words in changed paths too, groups sibling files, and says when the sample is small. `recipe --check` says when it can't judge.
- `drift` checks naming affixes (`_cents`, `is_`) and has fewer false alarms on doc vocabulary, labels, and import paths.
- `ledger check --diff` matches claims per sentence, catching paraphrases without flagging paragraphs that only share the topic.
- `yuzu check` covers terraform, tofu, terragrunt, pulumi (`infra_dirs`), and gcloud, az, doctl, flyctl, heroku, vercel, netlify, wrangler (`cli_tools`).
- `yuzu probe` names a certificate both sides reject as the server's problem, with a fix matched to curl's reason.
- `recipe` needs 6 earlier changes to a file before it predicts anything, up from 5: higher precision on all four repos measured.
- Release builds use fat LTO, one codegen unit, and `panic = "abort"`: 29% smaller and about 10% faster.
