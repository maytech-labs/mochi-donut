# Evals

Labeled answers for matcha's commands, scored with `matcha eval`. Run them after changing a heuristic:

```sh
matcha eval evals/*.json
```

Each case pins a repo to a commit, runs a command that prints JSON, and compares its findings with ones a person labeled `real` or `false`, or checks exact values with `expect`. A case fails when precision or recall drops below its gate, or a value differs.

These cases point at clones under `~/projects/software/personal/` (Anchor is private), so they run on the owner's machine, not in CI. To add a repo, clone it, add a case with its commit, run `matcha eval <spec> --record` to list what it finds, and move each finding into `real` or `false`.

| Spec | Cases |
|---|---|
| `drift.json` | Anchor before and after its guidelines (91% precision, 100% recall), Ghostty (no drift), Ghidra (37%: its docs name a lot of code outside the repo) |
| `census.json` | Anchor's production Rust has no `unwrap()`; lockfiles don't count; a too-narrow pattern points at its word |
| `recipe.json` | `--like migration` finds commits by the paths they changed |
