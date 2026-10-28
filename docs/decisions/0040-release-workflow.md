# 0040. Tagged releases publish prebuilt binaries and bump the tap

- **Status:** Accepted
- **Date:** 2026-10-28

## Context

The formula in `maytech-labs/homebrew-tap` is HEAD-only ([0039](0039-org-homebrew-tap.md)), so every install builds all four tools with Rust and fat LTO ([0009](0009-release-profile.md)). real-check and arm-tools already publish prebuilt archives and bump their formulas from a release workflow.

## Decision

- Pushing a tag like `v0.1.0` runs `.github/workflows/release.yml`, modeled on real-check's. The tag must match the workspace version in `Cargo.toml`.
- It builds for `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, and `aarch64-unknown-linux-gnu`. Each archive holds `matcha`, `yuzu`, `ube`, `taro`, `LICENSE.md`, `THIRD-PARTY-NOTICES.md`, `README.md`, and `CHANGELOG.md`, with a sha256 beside it.
- The release notes are the version's section of `CHANGELOG.md`.
- A last job points each url and sha256 in `Formula/mochi-donut.rb` in the tap at the new archives. It pushes with `HOMEBREW_TAP_TOKEN`, a fine-grained token with contents write on the tap only, owned by `maytech-labs`.
- `THIRD-PARTY-NOTICES.md` holds the licenses of every crate the binaries link, which they require with each copy. `cargo xtask notices` writes it from `Cargo.lock`, and `cargo xtask check` fails when it's out of date.

## Consequences

- `brew install mochi-donut` downloads binaries instead of building; `--HEAD` still builds `main`.
- Before the first release, the formula in the tap needs a `url` and `sha256` per platform for the bump job to replace. Merge that change just before pushing the first tag.
- A dependency change means rerunning `cargo xtask notices`.
- The token expires; the org caps fine-grained token lifetimes, and the job fails with a 403 or a lifetime error when it lapses.

## Alternatives considered

- **Stay HEAD-only.** No release machinery, but every install needs Rust and minutes of compiling.
- **A GitHub App instead of a token.** Nothing to renew, but another thing to set up for one push per release. Worth it if the token keeps lapsing.
