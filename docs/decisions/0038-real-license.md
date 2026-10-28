# 0038. The Reciprocal Ethical AI License (REAL) 1.0, not MIT

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

The repo is about to go public under `maytech-labs`. [0005](0005-mit-license.md) chose MIT, which lets anyone train a closed model on the code. The owner wants the code shared freely but not taken into closed AI training, and wants changes to stay shared.

## Decision

- The suite is licensed under [REAL 1.0](https://github.com/maytech-labs/real-license), copied unchanged into `LICENSE.md` with the copyright line filled in.
- REAL has no SPDX id, so the metadata points at the file: `license-file = "LICENSE.md"` in the workspace `Cargo.toml` (each crate inherits it), `license :cannot_represent` in the Homebrew formula, and `"LicenseRef-REAL-1.0"` in each plugin's `plugin.json`.
- Contributions are accepted under the same license (inbound equals outbound), with no CLA.
- Docs call the project source-available, not open source, because REAL isn't OSI-approved.

## Consequences

- Modified source must be shared under REAL. Training a model on the code requires releasing its weights and training pipeline under REAL, and some uses (mass surveillance, weapons) are forbidden.
- Homebrew core and some distributions won't accept a non-OSI license. This repo is its own tap ([0011](0011-homebrew-tap.md)), so that doesn't block installs.
- crates.io accepts `license-file`, but tools that read SPDX ids (cargo-deny, license scanners) will flag the crates as unknown. Projects with strict license policies may not depend on them.
- Contributors who only know MIT may not expect the copyleft terms; CONTRIBUTING.md and the README say what they mean.

## Alternatives considered

- **Keep MIT:** simplest and most widely accepted, but allows exactly the closed training the owner wants to prevent.
- **AGPL-3.0:** OSI-approved network copyleft, but says nothing about model training or weights.
- **An OpenRAIL license:** has use restrictions, but is aimed at models rather than source code and has no weight reciprocity.
