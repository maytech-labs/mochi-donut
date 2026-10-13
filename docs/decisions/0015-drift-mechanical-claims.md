# 0015. drift checks mechanical claims first, and stays quiet when unsure

- **Status:** Accepted, amended by [0021](0021-usability-from-trying-the-tools.md)
- **Date:** 2026-10-12

## Context

The design splits `drift` into mechanical claims, which come from the text with no help, and rule claims, which need a check comment beside the sentence. Its bar: run on Anchor's docs as they were before the guidelines PR, flag the `ClientId(Uuid)` newtype claim in `engineering/conventions.md`. It leaves open which code spans count as claims, how to tell a claim from an example, and which docs to read.

## Decision

Build mechanical claims now. Rule claims (`<!-- matcha: no-match ... -->`), `--suggest`, and counts beside a path ("13 ADRs") come later.

- **Claims checked:**
  - **Links** to files in the repo must resolve, and `#anchors` into Markdown must match a heading (GitHub's anchor rules) or an `id`/`name` attribute.
  - **Code spans that read as paths** must exist relative to the doc, relative to the root, or as the tail of a tracked path (`lib/money.ts` for `frontend/src/lib/money.ts`). A span reads as a path if its last part has a known extension, it ends in `/`, or its first segment is a tracked directory.
  - **Code spans that read as identifiers** must appear as a word in some source file (anything tracked but `.md`, `.mdx`, `.txt`, `.rst`). An identifier is CamelCase, snake_case with a lowercase letter, a call (`refresh()`), a macro (`query!`), or a path (`Clock::today`). Only the last segment is checked: earlier ones are often other crates' names (`Local::now`) that the repo never spells out. Plain lowercase words and ALL_CAPS names are skipped: they're as likely English or environment variables.
  - **`make <target>`** must be a target in a tracked Makefile, and **`npm run <script>`** a script in a tracked `package.json`.
- **Not claims:** spans in a clause that negates them ("`a`, not `b`", "rather than", "was removed", "once it exists", "not yet"), after "e.g." or "for example", on an unchecked `- [ ]` item, or containing placeholder words (`foo`, `example`, `YYYY`, `NNNN`, `<...>`). Fenced code blocks and HTML comments are skipped.
- **Docs read:** tracked `.md` and `.mdx`, except test fixtures (`testdata/`, `test/data/`, `fixtures/`), vendored code (`vendor/`, `third_party/`, `node_modules/`), and any directory below the root with its own `LICENSE` or `COPYING` file, which is usually someone else's project.
- **Past and future docs** (changelogs, release notes, `decisions/`, `adr/`, roadmaps, `requirements/`, `rfcs/`, `proposals/`) get their links checked but not their code spans. They name things that are gone, or not built, on purpose.
- **Escape hatches:** `<!-- matcha: skip -->` skips the next line. `<!-- matcha: links-only -->` anywhere in a doc limits it to link checks. This repo's `docs/design.md` uses it: it's a proposal, and its examples quote Anchor.
- **Missing paths that git ignores** (`.env`, `target/`) are skipped, since they may exist on other machines. `git check-ignore` runs once per 500 such paths, not once per claim.
- **Exit 1 when any claim fails.** The design says `drift` starts as a report in CI; that's a choice for the CI job (`|| true`), not for the exit code contract.
- **Symbols are looked up by scanning source files for just the names the docs claim**, not by indexing every identifier. No new dependencies; matcha grew from 1,612 KB to 1,712 KB.

## Results (2026-10-12)

| Repo | Docs | Claims checked | Fail | Real drift (judged by hand) | Time |
|---|---|---|---|---|---|
| Anchor, before the guidelines PR | 38 | 377 | 11 | 10 | 0.4 s |
| mochi-donut | 25 | 77 | 0 | | 0.4 s |
| Ghostty | 20 | 60 | 0 | | 0.5 s |
| IntelliJ Community | 108 | 581 | 50 | about 31 | 48 s |
| Ghidra | 149 | 727 | 19 | about 7 | 1.6 s |

- On Anchor it flags `conventions.md:13`, `ClientId(Uuid)`: "`ClientId` appears in no source file (475 searched)". The bar is met. The other real ones: `Suspense`, which the frontend never uses; `acord::render_25`, with no acord crate; three data-model tables and four background jobs the code doesn't have. The one doubtful case is a config key named in an open question.
- On the big repos it found renamed and removed classes, a typo'd class name (`WizardSerivce`), a typo'd method (`setHeadlessContinationOption`), moved files, and anchors to renamed headings.
- Links, paths, and anchors are nearly always right. The false alarms are symbols that name things outside the repo: functions in a binary a tutorial analyzes, a third-party library, example project names (`Project1`).
- Time is mostly reading files. IntelliJ's 48 s matches `census` on the same 225,000 files; that's the persistent index's trigger from ADR 0007, not drift's.

## Consequences

- `drift` misses claims it can't classify: about two of every three code spans and links are skipped. The `skipped` count says so in every run.
- Symbol checks are only as precise as the docs are about what's local. Repos whose docs name a lot of outside code will need `skip` comments or a `links-only` doc, or should run it as a report.
- Mentioning a name anywhere in source, even a comment, counts as the name existing. That misses some drift, and never invents it.

## Alternatives considered

- **Check every segment of a symbol path:** catches more (`OldType::new()`), but flags external crates' types the repo only uses through a re-export or a prelude.
- **Index every identifier once:** simpler, but allocates a string for every distinct word in the repo. Scanning for the claimed names allocates only for the ones it finds.
- **A Markdown parser crate** (`pulldown-cmark`): handles more edge cases, but drift needs only code spans, links, and headings, line by line, and the line numbers are the evidence.
- **Ask git about ignored paths per claim:** correct, but one process per missing path.
