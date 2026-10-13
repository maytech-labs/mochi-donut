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
- Design doc and ADRs 0001–0018.
- `make install` (default `/usr/local/bin`) and a Homebrew formula (`brew install --HEAD mochi-donut` from this repo's tap).

### Changed

- `recipe` needs 6 earlier changes to a file before it predicts anything, up from 5: higher precision on all four repos measured.
- Release builds use fat LTO, one codegen unit, and `panic = "abort"`: 29% smaller and about 10% faster.
