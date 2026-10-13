# Using taro (planned)

<!-- matcha: links-only -->

**Proposed, not built.** This is the intended interface, to review before building. It follows the shared conventions: `--json` on every command; exit 0 when nothing's wrong, 1 when a check failed, 2 for bad usage. Every command talks to the forge, through `gh` or `glab`.

```sh
taro ready                       # the current branch's PR
taro ready 42                    # PR 42; exit 1 with each blocker
taro reviews 42                  # open threads as a checklist
taro owners --diff               # owners and suggested reviewers for the change
taro owners crates/yuzu/src/     # for paths
taro flaky --since 30d           # checks that fail then pass on the same commit
taro cleanup                     # branches proved merged, and ones that aren't
taro cleanup --delete            # delete the proved ones, after asking
```

| Flag | Commands | |
|---|---|---|
| `--repo <HOST/OWNER/REPO>` | all | Instead of the push remote's |
| `--since <AGE>` | flaky, owners | How much history to read |
| `--remote` | cleanup | Include remote branches, not only local |
| `--delete` | cleanup | Delete the branches proved merged, after asking |
