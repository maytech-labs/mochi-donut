# 0009. A release profile tuned for small and fast

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

`yuzu check` runs before every shell command and `matcha recipe --check` before every commit, so start-up and run time matter, and smaller binaries install and download faster. The owner asked for binaries as small and fast as possible.

## Decision

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
```

Measured on an Apple silicon Mac, Rust 1.98.1, median of 5 runs: `recipe --replay 200` on Ghostty (CPU-heavy) and `census --spelling cancel` on Anchor (I/O-heavy).

| Profile | matcha | yuzu | replay | census |
|---|---|---|---|---|
| thin LTO, strip (before) | 2207 KB | 710 KB | 433 ms | 45 ms |
| fat LTO, 1 codegen unit | 1785 KB | 571 KB | 389 ms | 40 ms |
| **+ panic = "abort" (chosen)** | **1574 KB** | **506 KB** | **388 ms** | **40 ms** |
| + opt-level = "s" | 1300 KB | 458 KB | 448 ms | 42 ms |
| + opt-level = "z" | 1126 KB | 410 KB | 647 ms | 64 ms |

## Consequences

- 29% smaller and about 10% faster than before. `yuzu --version` starts in about 3.5 ms.
- Release builds take longer (one codegen unit, fat LTO). Debug builds and tests are unaffected.
- A panic aborts with no unwinding. The tools don't catch panics, so nothing changes for users.

## Alternatives considered

- **`opt-level = "s"` or `"z"`:** 17 to 28% smaller again, but 15 to 50% slower. Speed matters more for hooks that run on every command.
- **Trimming clap's default features** (colors): saves 16 KB (1%), and loses colored help. Not worth it.
- **Rebuilding `std` with `build-std`:** smaller still, but needs nightly Rust.
