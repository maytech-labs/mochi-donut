# 0002. Rust, built with Cargo, not Bazel

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

The suite needs fast, single-binary command-line tools that start in milliseconds (`yuzu check` runs before every shell command) and install easily on macOS, Linux, and Windows. The owner works in Rust and also uses Bazel in other repos.

## Decision

- Everything is Rust, in one Cargo workspace: `crates/matcha`, `crates/yuzu`, `crates/index`, `crates/common`, and `xtask` for repo automation (`cargo xtask <task>`).
- No Bazel.
- The toolchain is pinned in `rust-toolchain.toml`.
- Releases will use `cargo-dist` for binaries and a Homebrew tap (set up with the first release).

## Consequences

- Contributors need only rustup. `cargo build`, `cargo test`, and `cargo install` work as every Rust developer expects.
- Repo automation is Rust too, so there's no Makefile or shell scripts to keep portable.
- If the suite gains a large non-Rust part, or the build gets slow enough to want remote caching, we revisit this.

## Alternatives considered

- **Bazel with `rules_rust`.** Pays off for large multi-language repos with remote caching. Here it adds a second dependency system on top of Cargo, longer setup, and a build most open-source Rust contributors don't know, for no gain in a pure-Rust workspace.
- **`just` or `make` for automation.** Fine tools, but one more thing to install; `cargo xtask` needs nothing beyond Cargo.
