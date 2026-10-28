# 0039. The formula lives in the maytech-labs tap

- **Status:** Accepted
- **Date:** 2026-10-27
- **Supersedes:** [0011](0011-homebrew-tap.md)

## Context

This repo was its own tap ([0011](0011-homebrew-tap.md)), so installing took `brew tap maytech-labs/mochi-donut https://github.com/maytech-labs/mochi-donut`. maytech-labs now has a shared tap, `maytech-labs/homebrew-tap`, which already carries arm-tools.

## Decision

- `Formula/mochi-donut.rb` moves to `maytech-labs/homebrew-tap`, unchanged. This repo no longer has a `Formula/` directory.
- Installing is `brew tap maytech-labs/tap`, `brew trust maytech-labs/tap`, `brew install --HEAD mochi-donut`.
- It stays HEAD-only until the first release. That release adds a `url` and `sha256` to the formula in the tap, and a release workflow can bump it there, as arm-tools' does.

## Consequences

- A shorter tap command, and one tap for every maytech-labs tool.
- Formula changes are made in the tap, not here. CI here no longer covers `Formula/`.
- Anyone who tapped this repo keeps a formula that still builds `main`, but it won't change again. The changelog says to switch.

## Alternatives considered

- **Keep this repo as its own tap.** One repo, but a longer install command, and a different one from the other maytech-labs tools.
- **Both taps.** `brew install mochi-donut` is ambiguous for anyone with both tapped.
