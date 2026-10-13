# 0024. matcha eval: labeled answers in spec files, for any command that prints JSON

- **Status:** Accepted
- **Date:** 2026-10-13

## Context

ADRs 0015 to 0021 tuned drift, census, recipe, and ledger by running them on real repos and judging every finding by hand. Each change risked undoing an earlier one, and the judgments lived only in ADR prose. The owner asked for this to be general, not tied to Cargo or to this repo.

## Decision

- **`matcha eval <spec.json>...`** runs each case: check out `repo` at `rev` in a temporary clone (`git clone --shared` for a local path, `--filter=blob:none` for a URL; deleted after), run `run` (any command that prints JSON; `matcha` and `yuzu` mean the binaries beside this one), and score its output.
- **Two kinds of check:**
  - **Labeled findings:** `findings` is a JSON pointer to an array, `key` names each finding from its fields (`{doc}:{line} {claim}`), and `real` and `false` list what a person judged. The result is precision (real out of labeled findings), recall (found out of labeled real), and the missed and unlabeled ones. `min_precision` and `min_recall` are gates.
  - **Exact values:** `expect` maps JSON pointers to values the output must have.
- **`--record`** writes findings nobody has labeled into the case's `unlabeled` list, for a person to sort into `real` or `false`.
- **JSON specs**, not TOML: matcha already parses JSON, and TOML would add about 300 KB for a file people edit rarely. Unknown fields are errors, so a misspelled gate can't silently turn off.
- **Exit 1 when any case fails** a gate or a value, 2 when a spec can't be read.
- **`evals/`** holds this repo's cases: drift on Anchor before and after its guidelines (91% precision, 100% recall), Ghostty (no drift), and Ghidra (37%); census on Anchor (no `unwrap()` outside tests, lockfiles skipped, a narrow pattern pointed at its word); recipe on Anchor (`--like migration` by paths). They point at local clones, one of them private, so they run on the owner's machine, not in CI.

## Consequences

- A heuristic change is judged by a number from labeled data, and a regression fails a gate.
- Any tool that prints JSON can be scored the same way, yuzu's included.
- Labels go stale when a repo is re-pinned; `--record` and `missed` show what moved.

## Alternatives considered

- **Snapshot tests of whole outputs:** catch every change, wanted or not, and say nothing about whether a change made answers better.
- **A `cargo xtask` task:** ties evaluation to this repo's build, which the owner ruled out.
