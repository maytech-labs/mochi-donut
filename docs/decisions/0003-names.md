# 0003. Names from mochi donuts: matcha for the repo, yuzu for the environment

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

The design used the placeholders `scout` (the repo tool) and `whereami` (the environment guard). The owner names projects after mochi donut flavors. The names need to be short to type, distinct, and available as packages.

## Decision

- **Suite:** `mochi-donut`. Each tool is named after a mochi donut flavor; future tools take new flavors (ube, hojicha, black sesame).
- **`matcha`** is the repo tool (`census`, `recipe`, `drift`, `ledger`): slow, steeped, deep knowledge.
- **`yuzu`** is the environment guard (`status`, `check`, `probe`): sharp, and wakes you up before you act.
- **Packages:** `yuzu` and `matcha` are taken on crates.io (an LLM command-line tool and a string-search library), so the packages are `mochi-yuzu`, `mochi-matcha`, `mochi-index`, and `mochi-common`, all free as of 2026-10-12. The commands installed are `yuzu` and `matcha`, the way the `ripgrep` package installs `rg`.
- The owner's earlier personal `yuzu` tool (a fact and work-item store) was retired the same day, so there's no conflict on their machine.

## Consequences

- Agents find the tools through hooks, tool descriptions, and trigger tables, not by guessing names (design doc, "How agents find the tools"), so the names don't need to describe the tools.
- Homebrew's unrelated `matcha` (an RSS digest tool) installs a `matcha` command. The README warns about it, and our Homebrew formula will be `mochi-matcha`.
- Web searches for "yuzu" mostly find the discontinued Nintendo Switch emulator. The README and repo description say "mochi-donut yuzu".

## Alternatives considered

- **Descriptive names** (`scout`, `whereami`): clearer at first sight, but generic, likely taken, and less memorable.
- **`yuzu` for the repo tool:** the retired `yuzu` stored facts, which is closest to `ledger`. But the guard's job (wake up before acting) fits yuzu's character better, and the old tool is gone.
