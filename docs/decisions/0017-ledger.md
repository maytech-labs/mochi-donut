# 0017. ledger: local per-branch files, evidence required, matched by shared words

- **Status:** Accepted, amended by [0021](0021-usability-from-trying-the-tools.md)
- **Date:** 2026-10-12

## Context

The design describes `matcha ledger`: claims marked verified (with evidence) or assumed, stored in `.matcha/ledger/<branch>.jsonl`, a hook that records commands as evidence, `check --diff` that matches assumed claims against added lines by text similarity, and a summary for the PR. It leaves open whether the ledger is committed, what "with evidence" requires, how similar is similar, and what the hook records.

## Decision

- **Commands:** `add <verified|assumed> "<claim>"`, `verify <id>` (assumed to verified), `list`, `check --diff [--base REV]`, `summary`, and `archive`. Promoting an entry to an ADR stays manual: an ADR needs context and alternatives the ledger doesn't hold.
- **Local, never committed.** `.matcha/ledger/<branch>.jsonl`, with `/` in branch names stored as `%2F`. The first write creates `.matcha/.gitignore` containing `*`, so the folder ignores itself without anyone editing the repo's `.gitignore`. Recorded command output can hold anything, so it shouldn't ride along in a commit. A detached `HEAD` uses `detached-HEAD`.
- **Verified means evidence.** `add verified` without `--evidence`, `--file`, or `--cmd` is a usage error (exit 2) that says to record it as assumed instead. `--file path:line` keeps the line's text, so the evidence survives later edits.
- **Entries are JSON lines** with an id, status, claim, evidence (command, output, file, line, text), and an RFC 3339 UTC time. A line that doesn't parse is an error naming the line, not a silently shorter ledger. `verify` rewrites the file through a temp file and a rename.
- **`check --diff`** reads lines the working tree adds against `HEAD` (or `--base`), staged or not, plus untracked files. It looks only at prose: lines in `.md`, `.mdx`, `.rst`, `.txt`, `.adoc`, and lines that start with a comment marker. A line states an assumed claim when it holds at least 60% of the claim's words and at least three of them (all of them for shorter claims). Words are lowercased, split at anything but letters and digits, with a short stop list and a trailing `s` dropped. Exit 1 when anything is flagged.
- **The hook** is `matcha hook claude-code`, a `PostToolUse` hook on Bash. It appends the command and an excerpt of its output (the first 1500 and last 500 characters) to `.matcha/ledger/<branch>.commands.jsonl`, keeping the file under about 1 MB. Values after `token`, `secret`, `password`, `api_key`, and similar keys are replaced with `[redacted]`. It prints nothing and always exits 0. `add verified --cmd <text>` attaches the latest recorded command containing the text. It takes about 17 ms per command.

  ```json
  {
    "hooks": {
      "PostToolUse": [
        { "matcher": "Bash", "hooks": [{ "type": "command", "command": "/usr/local/bin/matcha hook claude-code", "timeout": 10 }] }
      ]
    }
  }
  ```

  Unlike yuzu's hook it needs no fallback: if it fails, a command goes unrecorded, and nothing is allowed that shouldn't be.

## Consequences

- An agent can hand a reviewer "Verified: ..., Assumed: ..." from `matcha ledger summary`, with commands and output attached.
- The ledger is lost with the clone. Archiving keeps old ones; anything worth keeping longer belongs in an ADR or a doc.
- Word overlap misses paraphrases ("FEINs start with 00-" against "FEIN prefix is 00-" shares two words of four) and can match unrelated lines that reuse a claim's words. The three-word floor keeps the second rare. Measuring both needs real sessions, which is the design's bar anyway.
- The design's bar (agents use it unreminded in most sessions; assumed claims stop reaching merged docs unmarked) can only be judged after the hook and the skill have been in use for a while.

## Alternatives considered

- **Commit the ledger:** reviewers could read it in the PR, but command output would land in history, secrets and all. The summary carries what a reviewer needs.
- **Embeddings or fuzzy string distance for matching:** better at paraphrase, but a model or a dependency, and harder to explain in the output than "3 of 4 words".
- **Record every tool call, not just Bash:** file reads are already evidence by path and line, which `--file` covers.
