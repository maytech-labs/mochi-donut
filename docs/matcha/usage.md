# Using matcha

Every command takes `--json` (one JSON object on stdout) and `--repo <path>` (default: the repo containing the current directory). Exit codes are the same everywhere:

| Code | Meaning |
|---|---|
| 0 | Ran, and found nothing wrong |
| 1 | Ran, and a check failed: a missing companion file, a stale doc claim, an assumed claim in a doc, an eval below its gate |
| 2 | Bad usage, no repo, or a file it couldn't read |

`matcha agent-guide` prints a short version of this page, written for agents.

## census

Counts the ways the repo does something, by language, with the most typical files.

```sh
matcha census "form state" --alt 'useState<\w*Draft>' --alt 'useForm[<(]' --in frontend/src
matcha census --spelling cancel
matcha census unwrap --alt '\.unwrap\(\)' --no-tests
```

| Flag | |
|---|---|
| `[LABEL]` | What you're asking about, echoed back |
| `--alt <REGEX>` | One way of doing it; repeat for each alternative |
| `--spelling <STEM>` | Count every spelling of words starting with the stem instead |
| `--in <PATH>` | Only look under these paths; repeatable |
| `--examples <N>` | Examples per alternative (default 3) |
| `--no-tests` | Leave out test files (`tests/`, `*.test.ts`, `*_test.go`, ...) and Rust's inline `#[cfg(test)]` modules |

Lockfiles and minified bundles never count. Each alternative reports its files, matches, share, how many of its files are tests, and, when its longest literal word appears in more files than the pattern does, a hint that the pattern may be too narrow. Exit 0.

## recipe

Learns from history which files change together.

```sh
matcha recipe --like "add an endpoint"      # what similar past changes touched
matcha recipe --check                       # the working tree: anything usually changed with this that wasn't?
matcha recipe --check --commit HEAD         # a past commit, against the history before it
matcha recipe --replay 50                   # how --check would have done on the last 50 commits
```

| Flag | |
|---|---|
| `--like <TEXT>` | Find past commits whose subjects or changed paths match, and list what most of them changed |
| `--check` | Report files that usually change with the changed ones but didn't; exit 1 if any |
| `--commit <REV>` | With `--check`: check that commit |
| `--replay <N>` | Measure `--check`'s precision and recall on the last N commits |
| `--history <N>` | Commits to learn from (default 2000) |
| `--min-support <N>` | Earlier changes to a file before it predicts anything (default 6) |
| `--min-confidence <F>` | Share of those changes that included the companion (default 0.9) |
| `--min-lift <F>` | How much likelier than usual the companion must be (default 2) |
| `--max-files <N>` | Ignore commits that changed more files than this (default 300) |

<!-- matcha: skip -->
`--check` says how many changed files had enough history to judge, and "can't tell" when none did. A missing generated file comes with its usual fix, such as `make openapi`. `--like` warns when it found fewer than 3 similar commits.

## drift

Checks that the docs' claims still match the repo: links and anchors, code spans naming paths, identifiers, naming affixes, `make` targets, and `npm run` scripts, plus any check comments.

```sh
matcha drift                      # every tracked .md and .mdx
matcha drift --in docs/           # only these
matcha drift --suggest            # propose check comments for rule sentences; changes nothing
```

| Flag | |
|---|---|
| `--in <PATH>` | Only check docs under these paths; repeatable |
| `--suggest` | Propose `<!-- matcha: ... -->` check comments for sentences like "never X", each with whether the code follows it today |

Each failure names the doc and line, the claim, why it fails, and what to do. Exit 1 if any claim fails. Which docs and spans count as claims, and the comments that adjust it, are in [config.md](config.md#drift).

## ledger

A per-branch record of claims, each verified (with evidence) or assumed.

```sh
matcha ledger add verified "Traefik listens on all three nodes" --cmd "get svc"
matcha ledger add verified "The API caps pages at 100" --file src/pagination.rs:14
matcha ledger add assumed "FEINs in tests use a 00- prefix"
matcha ledger verify 3 --evidence "grep -r '00-' tests: 41 of 41"
matcha ledger list
matcha ledger check --diff                 # exit 1 if an added doc or comment line states an assumed claim
matcha ledger summary                      # Markdown for a PR description
matcha ledger archive                      # after the branch merges
```

| Command | Flags |
|---|---|
| `add <verified\|assumed> <CLAIM>` | `--evidence <TEXT>`, `--file <PATH[:LINE]>`, `--cmd <TEXT>`. Verified needs at least one (exit 2 otherwise). |
| `verify <ID>` | The same evidence flags |
| `check --diff` | `--base <REV>` (default `HEAD`); compares each added sentence in docs and comments with the assumed claims |
| `list`, `summary`, `archive` | |

`--cmd` attaches the latest command the hook recorded that contains the text, with its output (secrets redacted). It needs the hook: see [agents.md](agents.md#the-ledger-hook).

## eval

Scores a command's JSON answers against labeled ones.

```sh
matcha eval evals/*.json
matcha eval evals/drift.json --case anchor --record
```

| Flag | |
|---|---|
| `<SPEC>...` | Spec files; the format is in [config.md](config.md#eval-specs) |
| `--case <TEXT>` | Only cases whose name contains the text |
| `--record` | Write findings nobody has labeled into each case's `unlabeled` list |

Exit 1 if any case falls below its gates or an expected value differs; 2 if a spec can't be read. `--repo` is ignored: each case names its own repo.

## Serving agents

`matcha mcp` serves the commands as MCP tools on stdin and stdout, and `matcha hook claude-code` is the ledger's recorder. Both are described in [agents.md](agents.md).
