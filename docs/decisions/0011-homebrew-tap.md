# 0011. This repo is its own Homebrew tap

- **Status:** Superseded by [0039](0039-org-homebrew-tap.md)
- **Date:** 2026-10-12

## Context

The owner wants to install with Homebrew. Homebrew's usual convention is a separate `homebrew-<name>` repo, but `brew tap <user>/<name> <url>` accepts any git repo that has a `Formula/` directory. There's no tagged release yet (ADR 0004).

## Decision

- The formula lives in this repo at `Formula/mochi-donut.rb`. One formula installs both `matcha` and `yuzu`.
- Until the first release it's **HEAD-only**: it builds the latest `main` from source with Homebrew's Rust.
  ```sh
  brew tap trammel-may/mochi-donut https://github.com/trammel-may/mochi-donut
  brew install --HEAD mochi-donut
  ```
- It declares `conflicts_with "matcha"` (homebrew-core's RSS tool installs a `matcha` command too).
- At the first release, add a `url` and `sha256` for the tagged source, and later prebuilt binaries from `cargo-dist`. Moving to a separate `homebrew-mochi-donut` repo then would allow the shorter `brew tap trammel-may/mochi-donut`.

## Consequences

- One repo to maintain, and the formula changes in the same commit as the code it builds.
- Tapping clones the whole repo, not just the formula. Fine at this size.
- `brew install --HEAD` compiles from source, so the first install takes a minute or two.
- `brew upgrade --fetch-HEAD mochi-donut` picks up new commits.

## Alternatives considered

- **A separate `homebrew-mochi-donut` repo:** shorter tap command, but a second repo to keep in step before there's anything to release.
- **Submitting to homebrew-core:** needs a stable release and notability. Later, if ever.
