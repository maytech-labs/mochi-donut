# Configuring matcha

matcha works with no configuration. These are the few things a repo or a person can add.

## drift

### Which docs it reads

Tracked `.md` and `.mdx` files, except:

<!-- matcha: skip -->
- test fixtures (`testdata/`, `test/data/`, `fixtures/`) and vendored code (`vendor/`, `third_party/`, `node_modules/`, and any directory below the root with its own `LICENSE` or `COPYING`)
- for code spans only: docs that record the past or plan the future (changelogs, release notes, `decisions/`, `adr/`, roadmaps, `requirements/`, `rfcs/`, `proposals/`). Their links are still checked.

### Markers

A marker is an HTML comment at the start of a line (up to three spaces in). The same text inside backticks is just text.

| Marker | Effect |
|---|---|
| `<!-- matcha: skip -->` | The next non-blank line isn't checked |
| `<!-- matcha: links-only -->` | Anywhere in a doc: only its links are checked. For proposals and write-ups of other repos. |

### Check comments

A check comment on the line before a sentence holds the check that proves it:

```markdown
<!-- matcha: no-match 'Utc::now\(' except 'clock.rs' except '**/tests/**' -->
Never call `Utc::now()` outside a `Clock` implementation.
```

| Part | |
|---|---|
| `no-match '<regex>'` | Fails if the pattern matches anywhere searched |
| `match '<regex>'` | Fails if it matches nowhere |
| `in '<path>'` | Search only here; repeatable. Default: every source file (not docs) |
| `except '<path>'` | Leave this out; repeatable |

<!-- matcha: skip -->
Quotes are `'` or `"`; inside them only `\'` is an escape, so regexes keep their backslashes. A path is a directory or file (`backend`, `src/clock.rs`), a file name at any depth (`clock.rs`), or a glob (`*.sql`, `**/tests/**`). A comment that can't run fails, with the syntax in the message. `matcha drift --suggest` proposes these.

## Eval specs

A spec is a JSON file with a `cases` list. Unknown fields are errors.

```json
{
  "cases": [
    {
      "name": "drift: anchor before the guidelines PR",
      "repo": "~/projects/anchor",
      "rev": "2b0a01c",
      "run": ["matcha", "drift", "--json"],
      "findings": "/failing",
      "key": "{doc}:{line} {claim}",
      "real": ["docs/engineering/conventions.md:13 ClientId(Uuid)"],
      "false": ["docs/open-questions.md:34 allowed_emails"],
      "min_precision": 0.85,
      "min_recall": 1.0,
      "expect": { "/docs_checked": 38 }
    }
  ]
}
```

| Field | |
|---|---|
| `name` | Shown in the results |
| `repo` | A local path (`~` allowed; relative to the spec file) or a git URL |
| `rev` | Commit to check out in a temporary clone. Without it, the command runs in the repo as it is. |
| `run` | The command and arguments; it must print JSON. `matcha` and `yuzu` mean the binaries beside the one running. |
| `findings` | JSON pointer to the array of findings |
| `key` | How to name a finding from its fields; default: its JSON |
| `real`, `false` | Findings a person judged right or wrong |
| `unlabeled` | Written by `--record` |
| `min_precision`, `min_recall` | Gates, 0 to 1 |
| `expect` | Exact values by JSON pointer |

This repo's specs are in [../../evals/](../../evals/README.md).

## What's under `.matcha/`

| Path | Holds |
|---|---|
| `.matcha/.gitignore` | `*`, so the folder never reaches a commit |
| `.matcha/ledger/<branch>.jsonl` | The branch's claims (`/` in branch names is stored as `%2F`) |
| `.matcha/ledger/<branch>.commands.jsonl` | Commands the hook recorded, with output excerpts; kept under about 1 MB |
| `.matcha/ledger/archive/` | Archived ledgers |
