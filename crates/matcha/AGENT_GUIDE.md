matcha: what a repo knows, with evidence.

Status: pre-release. Commands marked (planned) exit with code 3 and say so.

When to run what
  Before copying a pattern or writing a guideline      matcha census "<what>"     (planned)
  Before a change you haven't made in this repo        matcha recipe --like "<what you're doing>"   (planned)
  Before committing                                    matcha recipe --check      (planned; meant to run from a hook)
  When you rely on a fact                              matcha ledger add verified|assumed "<claim>"   (planned)
  In CI                                                matcha drift               (planned)

Flags on every command
  --json         one JSON object on stdout
  --repo PATH    the repo to look at (default: the one containing the current directory)

Exit codes
  0  ran, nothing wrong      1  a check failed
  2  bad usage or no repo    3  not built yet
