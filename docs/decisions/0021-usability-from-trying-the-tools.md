# 0021. Changes from trying the tools on real questions

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

With every command built, each was used the way an agent would: on Anchor (now with its guidelines merged), this repo, Ghostty, Ghidra, and IntelliJ, and through the MCP server in a headless Claude Code session. Several answers were technically right and practically misleading. This records what changed, and amends ADRs 0012, 0015, 0017, and 0018.

## Decision

- **census leaves out lockfiles and minified bundles** (`Cargo.lock`, `package-lock.json`, `yarn.lock`, `go.sum`, `*.min.js`, and the like). On Anchor, 383 of 401 matches for `license` were in `package-lock.json`.
- **census says how many matching files are tests**, and `--no-tests` leaves them out, along with Rust's inline `#[cfg(test)]` modules. On Anchor, `.unwrap()` was 875 matches in 49 files; outside tests it's none, which is the repo's rule.
- **census flags a pattern narrower than its word.** When the pattern's longest literal word appears in more files than the pattern does, it says so: `useForm\(` found nothing, but `useForm` is in `ClientForm.tsx` as `useForm<Values>(`. An agent's first regex is often too narrow; that's how one concluded "no form library" in testing.
- **`recipe --like` keeps files a strict majority of similar commits changed** (with 2 commits, both), collapses three or more sibling files with the same count into their pattern, caps the text at 25 rows (`--json` has all), and says when it found fewer than 3 similar commits. It also matches query words against the changed paths, weighted half as much as the subject: "migration" found nothing on Anchor, whose subjects never say it, and now finds the 16 commits that touched `backend/migrations/`.
- **`recipe --check` says how many changed files it could judge.** With none that have 6 earlier changes, it says it can't tell, instead of "nothing missing". This amends ADR 0012's output, not its thresholds.
- **drift checks naming affixes** (`_cents`, `is_`): one holds when some identifier ends or starts with it. A **single capitalized word** (`Accepted`, `Suspense`) also holds when another doc uses it, so a doc vocabulary word isn't reported as a missing type. Spans ending in `:` are labels. "Drop" counts as a negation. A `../` path that doesn't resolve from the doc is matched by its tail, since it's often an import path. On Anchor's current docs this took failures from 33 to 11, of which 10 are real. This amends ADR 0015.
- **ledger matches claims per sentence**, not per line, and needs the shared words to be at least 40% of the words the sentence and claim use between them, besides three or more and half of the claim's. A Markdown paragraph is one line, and long paragraphs on the claim's topic matched; now paraphrases match and topic mentions don't. On this branch's 6,918 added lines it flags the one sentence that states an assumed claim. This amends ADR 0017.
- **probe has a rule for a certificate both sides reject**, with the fix chosen from curl's reason: renew an expired one, trust your own CA for a self-signed one, reissue one that doesn't cover the name. Before, it said to check whether the port was open. Only curl's first error line is kept. This amends ADR 0018.
- **MCP descriptions lead with the question they answer.** In a headless session, an agent asked "how is form state usually handled?" used seven grep and read calls and never census. With census described as "Use when asked how this repo usually does something ... One call instead of several greps", it called census first.

## Consequences

- Answers carry the qualifiers an agent needs (tests, sample size, files it couldn't judge, narrow patterns), at the cost of a few more lines.
- Agent uptake depends on wording as much as on the tool. One session each is anecdote, not measurement; the ledger's bar still needs real sessions.

## Alternatives considered

- **Leave tests in census and let the agent filter:** the agent can't tell which files are tests without a second pass.
- **Embeddings for ledger matching:** still not needed; per-sentence word overlap handles paraphrase and topic mentions on the cases tried.
