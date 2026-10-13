# matcha design

<!-- matcha: links-only -->
<!-- Examples quote Anchor and other repos, so `matcha drift` checks only this doc's links. -->

matcha answers questions about one repo from its own files, history, and docs, with evidence: counts, examples, commits, and sample sizes. The cross-tool design (why two tools, interfaces, discovery, rollout) is in [../design.md](../design.md); how to run it is in [usage.md](usage.md).

| Command | Question |
|---|---|
| [`census`](#census-how-is-this-usually-done-here) | How is this usually done here? |
| [`recipe`](#recipe-what-else-changes-when-i-change-this) | What else changes when I change this? |
| [`drift`](#drift-are-the-docs-still-true) | Are the docs still true? |
| [`ledger`](#ledger-what-did-i-verify-and-what-did-i-assume) | What did I verify, and what did I assume? |
| [`eval`](#eval-are-the-answers-still-right) | Are the answers still right? |

## The index

`matcha`'s four commands sit on one local index. Phase 1 builds it in memory from `git` on each run ([ADR 0007](../decisions/0007-phase-1-index.md)); the persistent form described here, under `.matcha/` (ignored by git) or a user cache directory, comes when repos need it:

- **Files:** paths, languages, sizes, generated or vendored markers.
- **History:** commits with their changed files, messages, and dates, from `git log --name-status`. Squash-merged PRs count as one change.
- **Text search:** backed by ripgrep's engine for speed.
- **Syntax-aware search:** tree-sitter grammars (through ast-grep's matcher), so a query can mean "functions that call `require_write`" rather than "lines containing `require_write`".

The index updates incrementally from git: new commits and changed files only.

Subcommands share flags: `--json`, `--repo <path>`, `--since <date>`, `--limit <n>`.

Optional repo configuration lives in `.matcha.toml`: named patterns and protected paths. Every command works without it.

## `census`: how is this usually done here?

**Problem.** Agents copy whatever code they happen to read first, so the minority pattern spreads, and guidelines get written from guesses.

**What it does.** Counts the ways the repo does something and shows the best example of each.

```
$ matcha census "form state" --in frontend/src/features
  draft in useState + <thing>FormModel.ts   19 files   features/holders/HolderForm.tsx
  react-hook-form + zod                      1 file    features/clients/ClientForm.tsx

$ matcha census --spelling cancel
  cancelled   98   (Rust 66, TypeScript 20, SQL 6, docs 6)
  canceled     0
```

**How it works.**
- A query is either a **named pattern** (from `.matcha.toml`, or a built-in set for common questions such as form state, error handling, and test layout) or a set of **alternatives** given on the command line (`--alt 'useForm(' --alt 'useState<.*Draft>'`).
- It counts matches per alternative by file and by area, and ranks examples by how typical they are: recent, short, and close to the asker's working directory.
- `--spelling <stem>` finds the spelling variants of a word across identifiers, strings, and docs.
- `--trend` shows whether each alternative is growing or shrinking over time, so "the old way" and "the new way" are clear.

**Output.** Each alternative with its count, its share, and up to three example locations. With `--json`, the same as data.

**Done when.** On Anchor, every claim in `docs/guidelines/` that describes a pattern can be confirmed or refuted by one `census` command.

**Status (2026-10-12): built.** `--alt` and `--spelling` work. On Anchor it confirmed the forms guideline (`\b\w+Draft\b` in 17 files, 94%, against `useForm` in 1) and the spelling rule (`cancelled` 105 times, `canceled` never) in under a second. Named patterns and `--trend` aren't built yet.

**Since (2026-10-13):** lockfiles and minified bundles are left out; each alternative says how many of its files are tests, and `--no-tests` leaves them out, along with Rust's inline `#[cfg(test)]` modules; a pattern narrower than its own word (`useForm\(` against `useForm<T>(`) says so. See [ADR 0021](../decisions/0021-usability-from-trying-the-tools.md).

## `recipe`: what else changes when I change this?

**Problem.** Many changes need companion edits that are easy to forget (regenerated files, registrations, tests). Repos keep hand-written checklists for them, and the checklists go stale.

**What it does.** Learns from history which files change together, and uses that two ways. (The first example's counts are illustrative; the second uses real numbers from Anchor's history.)

```
$ matcha recipe --like "add an endpoint"
  Based on 9 similar changes:
    backend/crates/anchor-api/src/routes/<resource>.rs     9/9
    backend/crates/anchor-api/src/openapi.rs               9/9
    frontend/src/api/openapi.json, schema.d.ts             9/9
    backend/tests/contract/<resource>.rs                   8/9
    frontend/src/api/queries/<resource>.ts                 7/9

$ matcha recipe --check            # against the working tree
  openapi.rs changed but frontend/src/api/openapi.json didn't.
    14 of 14 earlier changes to openapi.rs changed both.
    Usual fix: make openapi
```

**How it works.**
- **Co-change statistics:** for each pair of files (and each pair of path patterns, such as `routes/*.rs` and `tests/contract/*.rs`), how often they change in the same commit, as support and confidence. Patterns come from grouping paths that share structure.
- **Similar changes:** `--like` finds past commits by their message and their changed files, then reports what those commits had in common. With no message, it uses the current diff to find similar past changes.
- **Fix hints:** when a missing companion file is generated, `recipe` looks for the command that regenerates it: in the `Makefile` and `package.json` scripts, and in commit messages and docs that name both.
- `--check` runs before a commit and only reports rules with high confidence and enough history (defaults: 90% of at least 6 changes), so it stays quiet unless it's sure.

**Done when.** Replaying Anchor's last 20 feature PRs with the final commit's companion files removed, `recipe --check` names the missing files with at least 80% recall and almost no false alarms (precision of at least 90%).

**Status (2026-10-12): built, below the bar.** `--check`, `--commit`, `--like`, and `--replay` work. Measured with `--replay` at the defaults: 100% precision on Ghostty, 97% on vim, 85% on Anchor, and 78% on MoltenVK, against the 90% bar. Precision depends on how a repo commits more than on the scoring, so the bar is per repo. The recall bar turned out to need labeled companion files to measure fairly. Scoring and measurement are in [ADR 0008](../decisions/0008-recipe-scoring.md); the defaults and what didn't help are in [ADR 0012](../decisions/0012-recipe-support-six.md). Path patterns in `--check` aren't built yet.

**Since (2026-10-13):** `--like` also matches query words against changed paths, keeps files a strict majority of similar commits changed, groups sibling files into their pattern, and warns below 3 similar commits; `--check` says how many changed files had enough history to judge. See [ADR 0021](../decisions/0021-usability-from-trying-the-tools.md).

## `drift`: are the docs still true?

**Problem.** Docs make claims about code, and nothing checks them. When code changes, the doc keeps saying the old thing, and the next agent follows it.

**What it does.** Finds claims in the docs that can be checked against the code, checks them, and reports the ones that are no longer true.

Run against Anchor's docs as they were before the guidelines PR:

```
$ matcha drift
  docs/engineering/conventions.md:13
    "Use newtypes for IDs (`ClientId(Uuid)`)"
    FAILS: no type `ClientId` exists; IDs are `Uuid` in every crate
  docs/architecture/backend.md
    "`anchor-api::files::FileStore`"  OK
  (counts of checked and failing claims, then the details)
```

**How it works.**
- **Mechanical claims** come from the text with no help: code spans that name files, paths, functions, types, commands, and `make` targets must exist; links and anchors must resolve; numbers stated beside a path ("13 ADRs") must match.
- **Rule claims** ("never X outside Y", "every Z has W") are marked in the doc with a small comment that holds the check, so the check lives next to the sentence it proves:
  ```markdown
  <!-- matcha: no-match 'Utc::now\(|Local::now\(' except 'clock.rs' -->
  Never call `Utc::now()` outside a `Clock` implementation.
  ```
- **Suggesting checks:** `matcha drift --suggest` proposes check comments for sentences that look like rules, for a person or agent to review and accept. It never adds them silently.
- Runs in CI as a report first, and as a failing check once the false-alarm rate is low.

**Done when.** Run on Anchor's docs as they were before the guidelines PR, it flags the ID-newtype claim in `engineering/conventions.md`.

**Status (2026-10-12): mechanical claims built, and the bar is met.** Links, anchors, and code spans naming paths, identifiers, `make` targets, and `npm run` scripts are checked. On Anchor before the guidelines PR it flags `conventions.md:13`, `ClientId(Uuid)`, plus 10 other failures, 9 of them real drift. On Ghostty and this repo it finds nothing; on IntelliJ and Ghidra about 31 of 50 and 7 of 19 are real, and the rest are symbols naming code outside the repo. What counts as a claim, the `skip` and `links-only` comments, and the measurements are in [ADR 0015](../decisions/0015-drift-mechanical-claims.md). Check comments (`no-match` and `match`) and `--suggest` are built too; on Anchor the design's clock rule catches a contract test calling `Utc::now()` directly. Counts beside a path aren't built, because what they count is a guess. See [ADR 0016](../decisions/0016-drift-check-comments.md).

**Since (2026-10-13):** naming affixes (`_cents`, `is_`) are checked against identifiers; a one-word name another doc also uses counts as doc vocabulary; labels, import paths, and footnotes aren't claims. On Anchor's current docs: 11 failures, 10 real. Precision is pinned by `evals/drift.json`. See [ADR 0021](../decisions/0021-usability-from-trying-the-tools.md).

## `ledger`: what did I verify, and what did I assume?

**Problem.** Agents mix checked facts with guesses, then write both into docs and PRs with the same confidence. The next session can't tell which is which.

**What it does.** Keeps a per-task record of claims, each marked verified (with the evidence) or assumed, and flags assumed claims before they reach a commit.

```
$ matcha ledger add verified "Traefik listens on all three nodes" --evidence "kubectl get svc -A: 10.0.0.127,10.0.0.24,10.0.0.8"
$ matcha ledger add assumed "FEINs in tests use a 00- prefix"
$ matcha ledger check --diff
  docs/guidelines/testing/test-data.md:11 states an assumed claim:
    "FEINs use the 00- prefix"   (assumed, no evidence)
```

**How it works.**
- Entries live in `.matcha/ledger/<branch>.jsonl`: the claim, its status, the evidence (a command and an excerpt of its output, or a file and line), and a time.
- Agents add entries through the CLI or an MCP tool. A hook can record commands and outputs automatically, so evidence is attached without extra work.
- `check --diff` matches assumed claims against added lines in the diff (by text similarity) and lists the ones that appear in docs or comments.
- Distinct from ADRs: **decisions go in ADRs, facts go in the ledger.** A ledger entry that turns out to be a decision is promoted to an ADR.
- When a branch merges, its ledger can be summarized into the PR description ("Verified: ..., Assumed: ...") and archived.

**Done when.** Agents use it without being reminded in at least most sessions, and assumed claims stop appearing unmarked in merged docs.

**Status (2026-10-12): built; the bar needs real sessions.** `add`, `verify`, `list`, `check --diff`, `summary`, `archive`, and the `matcha hook claude-code` recorder work. The ledger is local and ignores itself, a verified claim must carry evidence, and `check --diff` flags an added doc or comment line that holds most of an assumed claim's words. See [ADR 0017](../decisions/0017-ledger.md). Whether agents use it unreminded can only be measured once the hook and skill are installed.

**Since (2026-10-13):** `check --diff` compares claims sentence by sentence and needs the shared words to dominate the sentence, so paraphrases match and long paragraphs on the same topic don't. `yuzu sessions` measures whether agents use it. See ADRs [0021](../decisions/0021-usability-from-trying-the-tools.md) and [0025](../decisions/0025-yuzu-sessions.md).

## `eval`: are the answers still right?

**Problem.** census, recipe, and drift run on heuristics. Each was tuned by running it on real repos and judging every finding by hand, and each change risked undoing an earlier one. The judgments lived only in ADR prose.

**What it does.** Scores any command that prints JSON against labeled answers, in a repo pinned to a commit.

```
$ matcha eval evals/*.json
ok   drift: anchor before the guidelines PR: 11 flagged; precision 91% (10 real, 1 false), recall 100%
ok   drift: ghidra: 19 flagged; precision 37% (7 real, 12 false), recall 100%
ok   census: anchor's production Rust never unwraps: 2 of 2 expected values match
```

**How it works.** Each case names a repo and a commit (checked out in a throwaway clone), the command, a JSON pointer to its findings, a key template, and the findings a person labeled `real` or `false`. It reports precision, recall, the missed and unlabeled findings, and fails a case below its `min_precision` or `min_recall`. `expect` checks exact values by JSON pointer. `--record` writes unlabeled findings into the spec for a person to sort. The spec format is in [config.md](config.md#eval-specs).

**Done when.** Every heuristic change is judged by `matcha eval` before it merges. **Status (2026-10-13): built.** `evals/` holds 8 cases for drift, census, and recipe; they point at local clones, so they run on the owner's machine, not in CI. See [ADR 0024](../decisions/0024-matcha-eval.md).

## Open questions

1. **The persistent index.** Everything is computed from git on each run. IntelliJ's 225,000 files take about 48 s to read; the persistent index of [ADR 0007](../decisions/0007-phase-1-index.md) is the fix once a repo crosses a second.
2. **Named census patterns and `--trend`** aren't built. Agents write their own regexes today, which the narrow-pattern hint helps with.
3. **recipe on squash-merged repos.** Big PRs blur which files change together; Anchor stays below the 90% bar.
