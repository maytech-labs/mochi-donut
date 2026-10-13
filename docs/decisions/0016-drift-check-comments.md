# 0016. drift check comments: `no-match` and `match`, and suggestions with evidence

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

ADR 0015 built drift's mechanical claims. The design's second half is rule claims: a comment beside a sentence like "never X outside Y" that holds the check proving it, and `--suggest` to propose those comments. It also lists counts beside a path ("13 ADRs"). It sketches one comment, `no-match '<regex>' except '<path>'`, and leaves the rest open.

## Decision

- **Two checks:** `no-match` (the pattern matches nowhere) and `match` (it matches somewhere). Together they cover "never X" and "we always use X". "Every Z has W" needs a notion of what a Z is, so it's left out.
- **Syntax:** `<!-- matcha: no-match '<regex>' [in '<path>']... [except '<path>']... -->`. Quotes are `'` or `"`. Inside them only `\'` is an escape, so a regex keeps its backslashes. A comment can span lines.
- **Paths** in `in` and `except` are a directory or file (`backend`, `src/clock.rs`), a file name at any depth (`clock.rs`), or a glob (`*.sql`, `**/tests/**`). With no `in`, the check searches source files only, not docs, so a sentence never matches itself.
- **Where a comment counts:** only when it starts its line (up to three spaces in), as an HTML block does in CommonMark, and never inside a code span. So a doc can show a check comment in backticks without running it. The `skip` and `links-only` markers from ADR 0015 follow the same rule.
- **Which sentence:** the next non-blank line after the comment. Failures are reported at that line, quoting it.
- **Check comments count in every doc**, including ADRs and `links-only` docs. They're explicit, so there's no question whether they're claims.
- **A comment that can't run fails** (unknown check, bad regex, unclosed quote), with the syntax in the fix. A check that silently passes would be worse than none (AGENTS.md rule 4).
- **Failures show the evidence:** the count of lines and files, and the first three `path:line`s.
- **`--suggest`** proposes a comment for a sentence that has a rule word ("never", "don't", "do not", "must not", "avoid", a leading "No"; or "always", "every", "must") followed, in the same sentence, by a code span that reads as an identifier. A path after "outside" or "except" becomes `except`. Each suggestion is run, and says whether the code follows it today. It never edits a doc.
- **Counts beside a path aren't built.** Whether "13 ADRs" next to `docs/decisions/` means every file, every `.md`, or every file but the template and README is a guess, and drift doesn't guess (AGENTS.md rule 3). A `match` comment can't count either; if counts turn out to matter, a `count` check that states its glob is the way.

## Results (2026-10-12)

- On Anchor's pre-guidelines docs, the design's own example (`no-match 'Utc::now\(|Local::now\(' except 'clock.rs'` above `conventions.md`'s clock rule) fails, at `backend/tests/contract/renewals.rs:413`: a contract test calls `Utc::now()` directly.
- `--suggest` on the same docs proposes 7 comments. Four hold today (`audit::record`, `operation_id`, `created_at`, `renewal_of_policy_id?`). `no-match 'unwrap\(\)'` fails with 830 lines, because the sentence's exceptions ("outside tests and startup") aren't paths; the reviewer adds them or drops it. Two are wrong readings of the sentence (`renews_term_id?`, `pull_request`), and the evidence shows it.

## Consequences

- A repo can turn any of its rules into a check that runs in CI, beside the sentence it proves.
- Regex checks are textual: a match in a comment or a string counts. `except` and `in` narrow it.
- Suggestions need review. They're a starting point with evidence attached, not checks to paste blindly.

## Alternatives considered

- **A separate rules file** (`.matcha.toml`): keeps docs clean, but the check drifts away from the sentence it proves, which is the problem drift exists to fix.
- **Structural checks** (ast-grep patterns): more precise, but a dependency per language, for a gain no measurement has asked for yet.
- **Suggesting from every sentence with a code span:** much noisier on Anchor (it proposed `no-match 'i64'` for "integer cents (`i64`), never floats").
