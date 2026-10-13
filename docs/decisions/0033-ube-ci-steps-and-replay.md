# 0033. ube reads CI by job and directory, measured by replaying merged changes

- **Status:** Accepted
- **Date:** 2026-10-13

## Context

The first bar in [docs/ube/design.md](../ube/design.md) asks whether, over the last 20 merged PRs of Anchor and this repo, `ube plan`'s checks would have caught every CI failure those PRs hit, in at most half the full suite's time. Running ube on Anchor, a repo with a Rust backend and a TypeScript frontend in subdirectories, showed it couldn't yet:

- CI's commands were planned at the repo root, but Anchor's CI runs them in `backend/` and `frontend/` (a step's `working-directory`, or a job's `defaults.run.working-directory`).
- The full suite missed every test command (`cargo llvm-cov`, `npm run coverage`) and `npm run format:check`, and counted `npx playwright install` as a check. `ube plan --all` would have passed without running a test.
- An npm package got only its `test`, `lint`, `typecheck`, and `check` scripts, so a change failing CI's `format:check` passed `ube done`.
- `backend/.sqlx/` and `backend/migrations/` belong to no Cargo package, so they sent the plan to the full suite.
- A `working-directory:` after a `run: |` block was read as part of the script.

## Decision

- **CI is read as jobs and steps.** Each command keeps its job and the directory it runs in; `.` is the root. An empty `run:` is a mapping (`defaults: run:`), not a script.
- **Checks include coverage** (`cargo llvm-cov`, `cargo tarpaulin`, `coverage` scripts) and scripts with `format` and `check` in their name. `npx playwright install` is setup.
- **A check that runs after a build in its job** (`npm run build`) gets that build run before it in the full suite, and isn't planned on its own for a package.
- **An npm package's plan adds the scripts CI runs in that directory,** except coverage (the same tests as `test`) and anything after a build.
- **In a Cargo workspace, outside every package,** `.sqlx/`, `migrations/`, `.cargo/`, and clippy or rustfmt config select `cargo test --workspace` and the workspace's lints.
- **The same check spelled two ways runs once,** compared by its words in any order without `--`, and the plan uses CI's spelling when CI has one.

## Results (2026-10-13)

Without GitHub access (gh's token was invalid), CI's actual failures couldn't be read. The replay used a stand-in: for each change, put its non-test, non-doc files back as they were before it, keep its tests, and run every planned and full-suite check on that tree and on the merged one. A check detects the regression when it passed on the merged tree and fails on the regressed one. An escape is a regression the full suite detects and the selection doesn't. Scripts and raw results stayed outside the repo.

| | Changes | Judged | Escapes | Fell back to the full suite | Selected / full time, when it didn't |
|---|---|---|---|---|---|
| mochi-donut | 20 | 6 | 0 | 3 | 46–53%, mean 50% |
| Anchor | 20 | 15 | 0 | 5 | 18–122%, mean 96% |

- Not judged: changes with no test change or no source change (13 here, 4 in Anchor), and one Anchor dependency upgrade the full suite couldn't detect either.
- Fallbacks were for `Makefile`, `Formula/`, plugin manifests, and `evals/` here, and `Dockerfile`, `ci.yml`, `.env.example`, and `dependabot.yml` in Anchor: build and CI config whose reach only the full suite covers.
- **The time bar fails on Anchor.** Most of its PRs touch `anchor-domain`, which every other crate depends on, and both halves of the app, so the selection is nearly the whole suite. Times also favor the full suite slightly: the selected checks ran first on each tree and paid the build.
- Old Anchor commits ran against today's `node_modules`, so their `typecheck` and `build` failed on the merged tree. Checks that failed there were left out of the judgment.

## Consequences

- No measured change slipped past the selection. A regression that removes a feature while keeping its tests is a weaker test than CI's real failures; the CI-history half of the bar still needs gh.
- On a full-stack repo with a shared core crate, package selection saves little. Saving more needs finer selection than packages, which [ADR 0029](0029-ube.md) put off because a miss is the worst outcome.
- CI parsing is still line-based. YAML anchors, matrix-substituted directories, and reusable workflows aren't followed.

## Alternatives considered

- **A YAML parser dependency:** exact, but a new dependency for a few keys; the line reader covers the workflows seen so far.
- **Count changes that only add tests:** reverting nothing tells nothing about selection.
- **Run the replay inside ube** (`ube replay`): useful later, but one measurement didn't justify a command.
