taro: the forge and the team. What a PR needs to merge, and what's safe to clean up.

When to run what
  Before asking to merge a PR        taro ready [pr]        every blocker, with what to do; exit 1 if any
  When addressing review feedback    taro reviews [pr]      open threads as a checklist
  When choosing reviewers            taro owners --diff     CODEOWNERS and who changed these files most
  Before debugging a CI failure      taro flaky             checks that fail then pass on the same commit
  After branches merge               taro cleanup           branches proved merged (merge, rebase, or squash)
                                     taro cleanup --delete  deletes them, after asking

Reading ready
  BLOCK items stop the merge; warn items are worth fixing. Conventions
  (changelog, ADR, Conventional Commits titles) come from the last 30 merged
  PRs, with the counts, and only once there are 10. If the required checks
  can't be read, every check counts. When GitHub says blocked for a reason
  taro can't see, the rules item says so.

Rules
  Don't merge or delete on your own judgment: report, and let the user act.
  cleanup never calls a branch safe without proof; trust its "keep" list.
  If gh can't authenticate, `yuzu whoami --verify` shows why. A stale
  GH_TOKEN overrides every stored login.

Flags on every command
  --json                   one JSON object on stdout
  --repo HOST/OWNER/REPO   the repo on the forge (default: the push remote's)

Exit codes
  0  nothing wrong      1  not ready, open threads, or a delete failed
  2  bad usage, no repo, or the forge couldn't be read
