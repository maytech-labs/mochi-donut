# Using ube (planned)

<!-- matcha: links-only -->

**Proposed, not built.** This is the intended interface, to review before building. It follows the shared conventions: `--json` on every command; exit 0 when nothing's wrong, 1 when a check failed, 2 for bad usage.

```sh
ube plan                         # checks for the working tree's changes, with reasons
ube plan --base main             # for everything since main
ube run                          # run the plan; failures in a few lines; exit 1 if any
ube run -- cargo test -p mochi-yuzu   # one command, parsed the same way
ube done                         # checklist: checks green on this tree, tests touched, no drift, no assumed claims
ube mutate                       # mutate changed lines, run their tests, list survivors
ube mutate --path crates/yuzu/src/wait.rs
```

| Flag | Commands | |
|---|---|---|
| `--base <REV>` | plan, run, done, mutate | Compare against this instead of `HEAD` |
| `--all` | plan, run | Plan the full suite, not a selection |
| `--rerun-failed` | run | Rerun only what failed last time on this tree |
| `--path <PATH>` | mutate | Only mutate lines in these files |
| `--max <N>` | mutate | Stop after N mutations |
