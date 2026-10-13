---
name: matcha
description: Use when asked how this repo usually does something, before copying a pattern or writing a guideline, before a change you haven't made in this repo (what else usually changes with it?), before trusting a doc or after changing code a doc describes (is it still true?), and whenever you check or assume a fact you'll write into a doc, comment, or PR (record it in the ledger).
---

# matcha: what this repo knows

matcha answers from the repo's own files and history, with counts and examples. Use the MCP tools when they're available (`census`, `recipe_like`, `recipe_check`, `drift`, `ledger_*`); otherwise run `matcha <command> --json`. `matcha agent-guide` prints the full guide.

## When to use what

| Moment | Tool | What you get |
|---|---|---|
| Choosing between two ways of doing something, or writing a guideline | `census` with one regex per alternative in `alts` | Files and matches per alternative, by language, with the most typical files |
| Unsure which spelling the repo uses | `census` with `spelling: "<stem>"` | Every spelling and its count |
| Starting a change you haven't made here | `recipe_like` with words from a commit subject | Past commits like it, and the files they changed together |
| Before committing | `recipe_check` | Files that usually change with yours but didn't, the counts, and the usual fix |
| Before trusting a doc, and after changing code it describes | `drift` | Links, paths, names, and check comments that no longer hold |
| Writing a rule into a doc ("never X") | `drift` with `suggest: true` | A check comment to put above it, and whether the code follows it today |
| You checked a fact | `ledger_add` with `status: "verified"` and evidence | A record a reviewer can trust |
| You're assuming a fact | `ledger_add` with `status: "assumed"` | A record, and a flag if you later write it into a doc unmarked |
| Before committing docs or comments | `ledger_check` | Added lines that state an assumed claim |
| Writing the PR description | `ledger_summary` | Verified and assumed claims as Markdown |

## Reading the results

- Report the counts as they are: "19 files use draft state, 1 uses react-hook-form" is the finding. Don't round it into "the repo always does X".
- `recipe_check` warnings come with "N of M earlier changes". A low M is weak evidence; say so.
- A `drift` failure on a name outside this repo is a false alarm: put `<!-- matcha: skip -->` on the line before it. A doc that's a plan or describes another repo can say `<!-- matcha: links-only -->`.
- Decisions go in ADRs. Facts go in the ledger. Verified needs evidence: what you ran and saw, or a file and line. If you didn't check, it's assumed.
