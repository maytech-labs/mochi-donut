ube: the change in hand. Which checks apply, and whether they passed on this code.

When to run what
  Before testing a change            ube plan                 the repo's own checks that apply, and why
  To test it                         ube run                  runs them; each failure in a few lines with a rerun command
  After a failure, once fixed        ube run --rerun-failed   just what failed in the last run
  Before saying it's done            ube done                 checks passed on THIS code, tests changed, no missing companion,
                                                              no doc made false, no assumed claim in a doc
  To see if the tests can fail       ube mutate               flips operators in changed lines; survivors are unproven lines

Reading results
  Results are tied to the exact code they ran on. Edit a file and `done` says
  the checks haven't run on this code: run them again. `plan` lists what it
  left out; `--all` runs the full suite (what CI runs). A passing run is
  recorded in matcha's ledger as evidence when matcha is installed.

Flags on every command
  --json         one JSON object on stdout
  --repo PATH    the repo (default: the one containing the current directory)
  --base REV     compare against where HEAD left REV (plan, run, done, mutate).
                 Default: where this branch left the default branch, so
                 committed work counts; HEAD on the default branch

Exit codes
  0  nothing wrong      1  a check failed, not done, or a mutation survived
  2  bad usage or no repo
