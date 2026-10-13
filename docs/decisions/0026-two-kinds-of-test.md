# 0026. Two kinds of test: unit tests drive the code, integration tests pin the contract

- **Status:** Accepted
- **Date:** 2026-10-13

## Context

AGENTS.md rule 8 said integration tests run the real binary and don't mock our own code, but not what each kind of test is for or when it may change. An audit after ADRs 0021 to 0025 found many user-visible behaviors (census skipping lockfiles, recipe matching paths, drift's affixes, yuzu check's infra coverage, `whoami --verify`, `wait ci`) covered only by unit tests. Unit tests change with the code, so they can't show a change didn't break what users rely on.

## Decision

- **Unit tests** sit beside the code. They drive it test-first and show each function does what it says. They're expected to change with the code.
- **Integration tests** sit in `crates/<tool>/tests/`. They run the built binary and judge it only by inputs and outputs, with side effects counted as outputs. They're long-lived: they change only when a contract genuinely changes, which is a breaking change. A change that passes them unedited introduced no regression.
- **No mocks of our own code.** Outside dependencies are injected through the seams the binary already reads (`PATH`, `--browser`, `HOME`, `XDG_*`, `GH_CONFIG_DIR`, `--dir`), so the code under test runs unchanged. For example, a stub `gh` on `PATH`.
- **Every command, flag, exit code, and user-visible behavior gets an integration test**, heuristics included.
- The rules are written out in AGENTS.md under Testing.

## Consequences

- Integration tests stay stable across refactors, and edits to them are reviewed as contract changes.
- Stubs for outside programs are small shell scripts written by tests at runtime, so the integration suite needs a Unix shell. CI already runs on Linux and macOS.

## Alternatives considered

- **One kind of test, tested at every level:** simpler to state, but either refactors churn every test, or edge cases are only reachable through the binary.
- **Mock gh, ssh, and aws inside the code (traits and fakes):** precise, but it adds seams that exist only for tests, and the test no longer runs the code users run.
