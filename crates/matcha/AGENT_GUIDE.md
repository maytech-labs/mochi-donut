matcha: what a repo knows, with evidence.

When to run what
  Before copying a pattern or writing a guideline
    matcha census "<what>" --alt '<regex>' --alt '<regex>'   compare ways of doing it
    matcha census --spelling <stem>                          which spelling the repo uses
  Before a change you haven't made in this repo
    matcha recipe --like "<words from a commit subject>"     files similar changes touched
  Before committing (best from a hook)
    matcha recipe --check                                    exit 1 if a usual companion file didn't change
  To decide whether to trust recipe here
    matcha recipe --replay 50                                its precision on this repo's history
  Not built yet (exit 3)
    matcha drift, matcha ledger

Reading recipe --check
  "X changed but Y didn't. 9 of 9 earlier changes to X changed both."
  The counts are the evidence. "Usual fix" names a make target or npm script
  that writes Y, when one exists. Thresholds: --min-support (6),
  --min-confidence (0.9), --min-lift (2).

Flags on every command
  --json         one JSON object on stdout
  --repo PATH    the repo to look at (default: the one containing the current directory)

Exit codes
  0  ran, nothing wrong      1  a check failed
  2  bad usage or no repo    3  not built yet
