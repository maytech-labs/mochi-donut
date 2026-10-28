# AGENTS.md

How to work in this repo, for AI agents and people. Read it before writing code.

## What this is

mochi-donut is a Cargo workspace of command-line tools for AI coding agents:

| Crate | Package | What it is |
|---|---|---|
| `crates/matcha` | `mochi-matcha` | The `matcha` binary: what a repo knows (`census`, `recipe`, `drift`, `ledger`, `eval`) |
| `crates/yuzu` | `mochi-yuzu` | The `yuzu` binary: what the user's environment is (`status`, `check`, `probe`, `whoami`, `wait`, `sessions`) |
| `crates/ube` | `mochi-ube` | The `ube` binary: the change in hand (`plan`, `run`, `done`, `mutate`) |
| `crates/taro` | `mochi-taro` | The `taro` binary: the forge and the team (`ready`, `reviews`, `owners`, `flaky`, `cleanup`) |
| `crates/index` | `mochi-index` | The local index of a repo's files and history that matcha reads |
| `crates/common` | `mochi-common` | Output format, exit codes, `--json` conventions, times, and the MCP server layer both tools share |
| `xtask` | (not published) | Repo automation: `cargo xtask <task>` |
| `Makefile` | | `make install` and friends: a thin wrapper over Cargo ([0010](docs/decisions/0010-make-install.md)) |
| `maytech-labs/homebrew-tap` | | The Homebrew formula lives in the org tap, not here ([0039](docs/decisions/0039-org-homebrew-tap.md)) |
| `plugins/`, `.claude-plugin/` | | Claude Code plugins (MCP servers, skills, hooks); this repo is their marketplace ([0019](docs/decisions/0019-mcp-and-plugin.md)) |

The design across tools is in [docs/design.md](docs/design.md); each tool's design, usage, and configuration are in `docs/<tool>/` ([index](docs/README.md)). Decisions are in [docs/decisions/](docs/decisions/).

## Working loop

1. Read `docs/<tool>/design.md` for what you're building, and any ADRs it links. Cross-tool questions are in `docs/design.md`.
2. Write the test first where practical: an integration test in `crates/<tool>/tests/` that runs the binary, then unit tests for the logic.
3. Run `cargo xtask check` (fmt, clippy with warnings as errors, all tests). CI runs the same on Linux and macOS.
4. Update docs in the same change: the tool's `docs/<tool>/` pages (design status, usage, config), the README's roadmap, and `CHANGELOG.md` under **Unreleased**. Run `matcha drift` to catch what you missed.
5. Record every decision you made as an ADR (below).

## Rules

1. **Rust only**, built with Cargo. No other languages or build systems without an ADR ([0002](docs/decisions/0002-rust-and-cargo.md)). The release profile in `Cargo.toml` is tuned and measured ([0009](docs/decisions/0009-release-profile.md)); changing it needs new measurements. Prefer small dependencies; check the binary size before adding one.
2. **Every command supports `--json`** (one object on stdout) and uses the exit codes in `mochi_common::exit`: 0 ok, 1 a check failed, 2 usage, 3 not built yet. These are a contract: agents and hooks act on them. Changing one is a breaking change.
3. **Evidence, not opinion.** Output shows counts, examples, commits, and sample sizes. When a tool doesn't know, it says so; it never guesses.
4. **Fail safe.** `yuzu check` treats anything unknown as risky. A crash or a parse error in a check blocks; it never silently allows.
5. **Nothing leaves the machine.** No telemetry, no network calls except those a command is explicitly for (`yuzu probe`).
6. **Output that teaches.** Every error and every failed check says what to do next.
7. **No `unsafe`** (forbidden workspace-wide). No `unwrap`/`expect` outside tests, except with a comment proving it can't fail.
8. **Two kinds of test, with different jobs.** Unit tests drive the code; integration tests pin the contract and run the real thing. See [Testing](#testing).

## Testing

Two kinds of test ([ADR 0026](docs/decisions/0026-two-kinds-of-test.md)). They have different jobs, so they follow different rules.

### Unit tests: does the code do what it says?

- **Where:** a `#[cfg(test)] mod tests` beside the code.
- **Job:** drive the code test-first, and show each function does what its doc comment says. Cover edge cases here; they're cheap.
- **How:** call one function or type directly with plain inputs. No processes, and no files unless the function is about files.
- **Lifetime:** short. They follow the code's shape, so they change when it does. Rewrite or delete them freely in a refactor.

### Integration tests: does the tool still keep its promises?

- **Where:** `crates/<tool>/tests/`.
- **Job:** pin the contract, everything a user or agent relies on. If code changes and these pass *without being edited*, nothing regressed.
- **How:** run the built binary (`Command::cargo_bin`) the way an agent would, and judge it only by inputs and outputs.
  - **Inputs:** arguments, stdin, environment variables, files, a git repo.
  - **Outputs:** stdout (prefer `--json`), the exit code, stderr, and side effects (files written, logs appended), which are outputs too.
- **No mocks of our own code.** Use real git repos in temp directories and real local servers. Replace what we don't control (gh, ssh, aws, a browser, the home directory, agent transcripts) through the seams the binary already reads, such as `PATH`, `--browser`, `HOME`, `XDG_*`, `GH_CONFIG_DIR`, and `--dir`. That's dependency injection at the process boundary: the code under test can't tell.
- **Assert what callers depend on:** JSON fields, exit codes, and the phrases a message must carry. Don't assert incidental layout, or the test churns.
- **Lifetime:** long. Change one only when the contract genuinely changes: a flag, a JSON field, an exit code, or a message agents act on. That's a breaking change, so the commit is marked `!` or an ADR says why.
- **Coverage:** every command, flag, exit code, and user-visible behavior has one, heuristics included. A behavior that only unit tests cover isn't protected from regressions.

## Recording decisions

Every time you make a decision, document it in `docs/decisions/`. The next agent has no memory of your session; the ADR is how it learns what you chose and why.

A decision needs an ADR when you chose between reasonable options and a future reader could ask "why is it like this?": a dependency, a data format, an algorithm or threshold, an interpretation of the design doc, a deviation from it, or a tradeoff. Ordinary code-level choices and following an existing pattern don't need one.

Copy `docs/decisions/0000-template.md` to the next number. Keep it short. ADRs are append-only: to change a decision, write a new one that supersedes the old, and change only the old one's status line.

## Writing

Plain, direct, and concrete, in docs, comments, CLI output, and commit messages. Short sentences, active voice, contractions. No em dashes, no hype, no exclamation points. Sentence case for headings. Name the file, the command, the number.

## Commits and pull requests

Commits follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) and stay terse ([ADR 0006](docs/decisions/0006-commit-messages.md)):

- **Subject only**, lowercase, imperative, no period, under about 50 characters: `feat(matcha): add census --spelling`.
- **Types:** `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`, `chore`. **Scopes** are crate names (`matcha`, `yuzu`, `index`, `common`, `xtask`) or left off.
- A body only when the why isn't obvious, and then a line or two.
- Breaking changes: `!` after the type (`feat(yuzu)!: ...`).
- **No tool attribution of any kind**: no `Co-Authored-By` trailers for AI tools, no "Generated with" lines, in commits or PR descriptions. The author is the person who owns the change.
- One logical change per commit. Work on a branch; `main` stays green.
- PR descriptions say what changed and why, which ADRs they add, and how it was tested.
