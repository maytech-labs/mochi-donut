# 0005. MIT license

- **Status:** Superseded by [0038](0038-real-license.md)
- **Date:** 2026-10-12

## Context

The owner wants credit for the work and no other restrictions on how people use it.

## Decision

The suite is licensed under the MIT License. Every crate's `Cargo.toml` says `license = "MIT"`, and contributions are accepted under the same license.

## Consequences

- Anyone can use, change, and redistribute the code, including commercially, as long as they keep the copyright notice. That notice is the credit.
- There's no patent grant, unlike Apache-2.0. That's acceptable for a small tools project.

## Alternatives considered

- **`MIT OR Apache-2.0`** (the Rust ecosystem default): adds Apache-2.0's patent grant, but also its longer terms and NOTICE-file conditions. More than the owner asked for.
- **Apache-2.0 alone:** the same tradeoff, without MIT's simplicity.
- **A copyleft license (GPL, MPL):** would require people who change the code to share their changes, which is a restriction the owner doesn't want.
